use std::{
    collections::{HashMap, HashSet},
    path::{Component, PathBuf},
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    vec,
};

use async_recursion::async_recursion;
use async_trait::async_trait;
use callisto::{
    entity_ext::generate_link, mega_commit, mega_refs, sea_orm_active_enums::RefTypeEnum,
};
use common::{
    errors::MegaError,
    utils::{self, ZERO_ID},
};
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::{EntryMeta, MetaAttached},
        object::{
            ObjectTrait, commit::Commit, signature::Signature, tree::Tree, types::ObjectType,
        },
        pack::{encode::PackEncoder, entry::Entry},
    },
};
use io_orbit::object_storage::MultiObjectByteStream;
use jupiter::{sea_orm::DatabaseTransaction, storage::Storage, utils::converter::FromMegaModel};
use tokio::sync::{RwLock, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    bus::{ApplicationEventHandler, TransportEvent},
    infra::cache::GitObjectCache,
    transport::{
        pack::RepoHandler,
        protocol::import_refs::{RefCommand, Refs},
    },
};

pub struct MonoRepo {
    pub storage: Storage,
    pub git_object_cache: Arc<GitObjectCache>,
    pub path: PathBuf,
    pub base_branch: String,
    pub from_hash: String,
    pub to_hash: String,
    // current_commit only exists when an unpack operation occurs.
    // When only a branch is updated and the pack file is empty, this value will be None.
    pub current_commit: Arc<RwLock<Option<Commit>>>,
    pub cl_link: Arc<RwLock<Option<String>>>,
    pub application: Arc<dyn ApplicationEventHandler>,
    pub username: Option<String>,
    /// Ref commands for this push (same role as on [`ImportRepo`](crate::transport::pack::import_repo::ImportRepo)).
    pub command_list: Mutex<Vec<RefCommand>>,
}

#[async_trait]
impl RepoHandler for MonoRepo {
    fn is_monorepo(&self) -> bool {
        true
    }

    fn save_entry_concurrency(&self) -> usize {
        self.storage.config().pack.save_entry_concurrency
    }

    fn sync_commands_after_unpack(&self, commands: &[RefCommand]) {
        *self
            .command_list
            .lock()
            .expect("command_list lock poisoned") = commands.to_vec();
    }

    async fn refs_with_head_hash(&self) -> (String, Vec<Refs>) {
        let storage = self.storage.mono_storage();

        let path_refs = storage
            .get_all_refs(self.path.to_str().unwrap(), false)
            .await
            .unwrap();

        let heads_exist = path_refs
            .iter()
            .any(|x| x.ref_name == common::utils::MEGA_BRANCH_NAME);

        let refs = if heads_exist {
            let refs: Vec<Refs> = path_refs.into_iter().map(|x| x.into()).collect();
            refs
        } else {
            let target_path = self.path.clone();
            let mut refs = vec![];

            let root_refs = storage.get_all_refs("/", true).await.unwrap();

            for root_ref in root_refs {
                let (tree_hash, commit_hash) = (root_ref.ref_tree_hash, root_ref.ref_commit_hash);
                let mut tree: Tree = Tree::from_mega_model(
                    storage.get_tree_by_hash(&tree_hash).await.unwrap().unwrap(),
                );

                let commit: Commit = Commit::from_mega_model(
                    storage
                        .get_commit_by_hash(&commit_hash)
                        .await
                        .unwrap()
                        .unwrap(),
                );

                for component in target_path.components() {
                    if component != Component::RootDir {
                        let path_compo_name = component.as_os_str().to_str().unwrap();
                        let path_compo_hash = tree
                            .tree_items
                            .iter()
                            .find(|x| x.name == path_compo_name)
                            .map(|x| x.id);
                        if let Some(hash) = path_compo_hash {
                            tree = Tree::from_mega_model(
                                storage
                                    .get_tree_by_hash(&hash.to_string())
                                    .await
                                    .unwrap()
                                    .unwrap(),
                            );
                        } else {
                            return (ZERO_ID.to_string(), vec![]);
                        }
                    }
                }
                let c = Commit::new(
                    commit.author,
                    commit.committer,
                    tree.id,
                    vec![],
                    &commit.message,
                );

                let new_mega_ref = mega_refs::Model::new(
                    &self.path,
                    root_ref.ref_name.clone(),
                    c.id.to_string(),
                    c.tree_id.to_string(),
                    false,
                );

                storage
                    .mega_head_hash_with_txn(new_mega_ref.clone(), c)
                    .await
                    .unwrap();

                refs.push(new_mega_ref.into());
            }
            refs
        };
        self.find_head_hash(refs)
    }

    async fn finalize_receive_pack(&self) -> Result<(), MegaError> {
        self.persist_mono_branch_cl_mega_refs_transaction().await?;
        self.traverses_tree_and_update_filepath().await?;
        let cl_link = self.cl_link.read().await.clone();
        self.application
            .handle(TransportEvent::MonoReceivePackFinalized {
                repo_path: self.path.clone(),
                base_branch: self.base_branch.clone(),
                from_hash: self.from_hash.clone(),
                to_hash: self.to_hash.clone(),
                username: self.username.clone(),
                cl_link,
            })
            .await
    }

    async fn save_entry(
        &self,
        entry_list: Vec<MetaAttached<Entry, EntryMeta>>,
    ) -> Result<(), MegaError> {
        let current_commit = self.current_commit.read().await;
        let commit_id = if let Some(commit) = &*current_commit {
            commit.id.to_string()
        } else {
            String::new()
        };
        let commit_models = self
            .storage
            .mono_service
            .save_entry(&commit_id, entry_list)
            .await?;

        if !commit_models.is_empty() {
            let commits_to_process: Result<Vec<(String, String)>, MegaError> = commit_models
                .into_iter()
                .map(|c| {
                    let model: mega_commit::Model = c.try_into()?;
                    let author_bytes = model.author.as_deref().unwrap_or("").as_bytes();
                    let signature = Signature::from_data(author_bytes.to_vec())?;
                    Ok((model.commit_id, signature.email))
                })
                .collect();

            self.storage
                .mono_storage()
                .process_commit_bindings(&commits_to_process?, self.username.clone().as_deref())
                .await?;
        }
        Ok(())
    }

    async fn update_pack_id(&self, temp_pack_id: &str, pack_id: &str) -> Result<(), MegaError> {
        let storage = self.storage.mono_storage();
        storage.update_pack_id(temp_pack_id, pack_id).await
    }

    async fn check_entry(&self, entry: &Entry) -> Result<(), GitError> {
        if self.current_commit.read().await.is_none() {
            if entry.obj_type == ObjectType::Commit {
                let commit = Commit::from_bytes(&entry.data, entry.hash).unwrap();
                let mut current = self.current_commit.write().await;
                *current = Some(commit);
            }
        } else if entry.obj_type == ObjectType::Commit {
            return Err(GitError::CustomError(
                "only single commit support in each push".to_string(),
            ));
        }
        Ok(())
    }

    // monorepo full pack should follow the shallow clone command 'git clone --depth=1'
    async fn full_pack(&self, want: Vec<String>) -> Result<ReceiverStream<Vec<u8>>, GitError> {
        self.incremental_pack(want, Vec::new()).await
    }

    async fn incremental_pack(
        &self,
        want: Vec<String>,
        have: Vec<String>,
    ) -> Result<ReceiverStream<Vec<u8>>, GitError> {
        let mut want_clone = want.clone();
        let pack_config = &self.storage.config().pack;
        let storage = self.storage.mono_storage();
        let obj_num = AtomicUsize::new(0);

        let mut exist_objs = HashSet::new();

        let mut want_commits: Vec<Commit> = storage
            .get_commits_by_hashes(&want_clone)
            .await
            .unwrap()
            .into_iter()
            .map(Commit::from_mega_model)
            .collect();
        let mut traversal_list: Vec<Commit> = want_commits.clone();

        // traverse commit's all parents to find the commit that client does not have
        while let Some(temp) = traversal_list.pop() {
            for p_commit_id in temp.parent_commit_ids {
                let p_commit_id = p_commit_id.to_string();

                if !have.contains(&p_commit_id) && !want_clone.contains(&p_commit_id) {
                    let parent: Commit = Commit::from_mega_model(
                        storage
                            .get_commit_by_hash(&p_commit_id)
                            .await
                            .unwrap()
                            .unwrap(),
                    );
                    want_commits.push(parent.clone());
                    want_clone.push(p_commit_id);
                    traversal_list.push(parent);
                }
            }
        }

        let want_tree_ids = want_commits.iter().map(|c| c.tree_id.to_string()).collect();
        let want_trees: HashMap<ObjectHash, Tree> = storage
            .get_trees_by_hashes(want_tree_ids)
            .await
            .unwrap()
            .into_iter()
            .map(|m| {
                (
                    ObjectHash::from_str(&m.tree_id).unwrap(),
                    Tree::from_mega_model(m),
                )
            })
            .collect();

        obj_num.fetch_add(want_commits.len(), Ordering::SeqCst);

        let have_commits = storage.get_commits_by_hashes(&have).await.unwrap();
        let have_trees = storage
            .get_trees_by_hashes(have_commits.iter().map(|x| x.tree.clone()).collect())
            .await
            .unwrap();
        for have_tree in have_trees {
            self.traverse(Tree::from_mega_model(have_tree), &mut exist_objs, None)
                .await?;
        }

        let mut counted_obj = HashSet::new();
        // traverse for get obj nums
        for c in want_commits.clone() {
            self.traverse_for_count(
                want_trees.get(&c.tree_id).unwrap().clone(),
                &exist_objs,
                &mut counted_obj,
                &obj_num,
            )
            .await;
        }
        let (entry_tx, entry_rx) = mpsc::channel(pack_config.channel_message_size);
        let (stream_tx, stream_rx) = mpsc::channel(pack_config.channel_message_size);
        let encoder = PackEncoder::new(obj_num.into_inner(), 0, stream_tx);
        encoder.encode_async(entry_rx).await.unwrap();
        // todo: For now, send metadata only for blob objects.
        for c in want_commits {
            self.traverse(
                want_trees.get(&c.tree_id).unwrap().clone(),
                &mut exist_objs,
                Some(&entry_tx),
            )
            .await?;
            entry_tx
                .send(MetaAttached {
                    inner: c.into(),
                    meta: EntryMeta::new(),
                })
                .await
                .unwrap();
        }
        drop(entry_tx);

        Ok(ReceiverStream::new(stream_rx))
    }

    async fn get_trees_by_hashes(&self, hashes: Vec<String>) -> Result<Vec<Tree>, MegaError> {
        Ok(self
            .storage
            .mono_storage()
            .get_trees_by_hashes(hashes)
            .await
            .unwrap()
            .into_iter()
            .map(Tree::from_mega_model)
            .collect())
    }

    async fn get_blobs_by_hashes(
        &self,
        hashes: Vec<String>,
    ) -> Result<MultiObjectByteStream<'_>, MegaError> {
        Ok(self.storage.git_service.get_objects_stream(hashes))
    }

    async fn get_blob_metadata_by_hashes(
        &self,
        hashes: Vec<String>,
    ) -> Result<HashMap<String, EntryMeta>, MegaError> {
        let models = self
            .storage
            .mono_storage()
            .get_mega_blobs_by_hashes(hashes)
            .await?;

        let map = models
            .into_iter()
            .map(|blob| {
                (
                    blob.blob_id.clone(),
                    EntryMeta {
                        pack_id: Some(blob.pack_id.clone()),
                        pack_offset: Some(blob.pack_offset as usize),
                        file_path: Some(blob.file_path.clone()),
                        is_delta: Some(blob.is_delta_in_pack),
                        // TODO: Populate `crc32` once mono blob metadata exposes it.
                        // For now we set it to `None` because `get_mega_blobs_by_hashes` does
                        // not provide CRC32 information for monorepo-backed packs.
                        crc32: None,
                    },
                )
            })
            .collect::<HashMap<String, EntryMeta>>();

        Ok(map)
    }

    async fn update_refs(&self, refs: &RefCommand) -> Result<(), GitError> {
        self.apply_cl_mega_ref_for_push_command(refs, None)
            .await
            .map_err(GitError::from)
    }

    async fn check_commit_exist(&self, hash: &str) -> bool {
        self.storage
            .mono_storage()
            .get_commit_by_hash(hash)
            .await
            .unwrap()
            .is_some()
    }

    async fn check_default_branch(&self) -> bool {
        true
    }

    async fn traverses_tree_and_update_filepath(&self) -> Result<(), MegaError> {
        let commit_guard = self.current_commit.read().await;
        let commit_opt = match commit_guard.as_ref() {
            Some(commit) => commit,
            None => {
                tracing::info!(
                    "Skipping file path update: no current commit available. \
                     This typically occurs when only updating references or pushing empty pack files."
                );
                return Ok(());
            }
        };

        let tree_hashes = vec![commit_opt.tree_id.to_string()];
        let trees = self
            .storage
            .mono_storage()
            .get_trees_by_hashes(tree_hashes)
            .await
            .map_err(|e| {
                MegaError::Other(format!(
                    "Failed to retrieve root tree for commit {}: {}",
                    commit_opt.id, e
                ))
            })?;

        if trees.is_empty() {
            return Err(MegaError::Other(format!(
                "Root tree {} not found for commit {}",
                commit_opt.tree_id, commit_opt.id
            )));
        }

        let root_tree = Tree::from_mega_model(trees[0].clone());

        tracing::info!(
            "Starting file path update for commit {} with root tree {}",
            commit_opt.id,
            commit_opt.tree_id
        );

        self.traverses_and_update_filepath(root_tree, PathBuf::new())
            .await
            .map_err(|e| {
                MegaError::Other(format!(
                    "Failed to update file paths for commit {}: {}",
                    commit_opt.id, e
                ))
            })?;

        tracing::info!(
            "Successfully completed file path update for commit {}",
            commit_opt.id
        );

        Ok(())
    }
}

impl MonoRepo {
    /// All branch commands update CL `mega_refs` in **one** DB transaction (same idea as import’s single-txn metadata commit).
    async fn persist_mono_branch_cl_mega_refs_transaction(&self) -> Result<(), MegaError> {
        let cmds = self
            .command_list
            .lock()
            .expect("command_list lock poisoned")
            .clone();
        let txn = self.storage.begin_db_transaction().await?;
        for cmd in &cmds {
            if cmd.ref_type == RefTypeEnum::Branch {
                self.apply_cl_mega_ref_for_push_command(cmd, Some(&txn))
                    .await?;
            }
        }
        txn.commit().await.map_err(MegaError::Db)
    }

    async fn apply_cl_mega_ref_for_push_command(
        &self,
        cmd: &RefCommand,
        txn: Option<&DatabaseTransaction>,
    ) -> Result<(), MegaError> {
        let storage = self.storage.mono_storage();
        let cl_link = self.fetch_or_new_cl_link().await?;
        let ref_name = utils::cl_ref_name(&cl_link);

        let (commit_hash, tree_hash) = if let Some(c) = &*self.current_commit.read().await {
            (cmd.new_id.clone(), c.tree_id.to_string())
        } else {
            // Empty pack / ref-only push: still materialize refs/cl/{link} from the target commit.
            let commit = storage
                .get_commit_by_hash(&cmd.new_id)
                .await?
                .ok_or_else(|| {
                    MegaError::Other(format!(
                        "Commit {} not found while writing CL ref {}",
                        cmd.new_id, ref_name
                    ))
                })?;
            (cmd.new_id.clone(), commit.tree)
        };

        let existing = match txn {
            Some(t) => storage.get_ref_by_name_in_txn(&ref_name, t).await?,
            None => storage.get_ref_by_name(&ref_name).await?,
        };

        if let Some(mut cl_ref) = existing {
            cl_ref.ref_commit_hash = commit_hash;
            cl_ref.ref_tree_hash = tree_hash;
            storage.update_ref(cl_ref, txn).await?;
        } else {
            let new_ref = mega_refs::Model::new(&self.path, ref_name, commit_hash, tree_hash, true);
            storage.save_refs(new_ref, txn).await?;
        }
        Ok(())
    }
}

impl MonoRepo {
    #[async_recursion]
    async fn traverses_and_update_filepath(
        &self,
        tree: Tree,
        path: PathBuf,
    ) -> Result<(), MegaError> {
        for item in tree.tree_items {
            let item_path = path.join(&item.name);

            if item.is_tree() {
                let tree_hash = item.id.to_string();
                let trees = self
                    .storage
                    .mono_storage()
                    .get_trees_by_hashes(vec![tree_hash.clone()])
                    .await
                    .map_err(|e| {
                        MegaError::Other(format!(
                            "Failed to retrieve tree {} at path '{}': {}",
                            tree_hash,
                            item_path.display(),
                            e
                        ))
                    })?;

                if trees.is_empty() {
                    return Err(MegaError::Other(format!(
                        "Tree {} not found at path '{}'",
                        tree_hash,
                        item_path.display()
                    )));
                }

                let child_tree = Tree::from_mega_model(trees[0].clone());

                self.traverses_and_update_filepath(child_tree, item_path.clone())
                    .await
                    .map_err(|e| {
                        MegaError::Other(format!(
                            "Failed to process subtree {} at path '{}': {}",
                            tree_hash,
                            item_path.display(),
                            e
                        ))
                    })?;
            } else {
                let blob_id = item.id.to_string();
                let file_path_str = item_path.to_str().ok_or_else(|| {
                    MegaError::Other(format!(
                        "Invalid UTF-8 path for blob {}: '{}'",
                        blob_id,
                        item_path.display()
                    ))
                })?;

                self.storage
                    .mono_storage()
                    .update_blob_filepath(&blob_id, file_path_str)
                    .await
                    .map_err(|e| {
                        MegaError::Other(format!(
                            "Failed to update file path for blob {} at '{}': {}",
                            blob_id, file_path_str, e
                        ))
                    })?;

                tracing::debug!(
                    "Updated file path for blob {} to '{}'",
                    blob_id,
                    file_path_str
                );
            }
        }

        Ok(())
    }
    async fn fetch_or_new_cl_link(&self) -> Result<String, MegaError> {
        let storage = self.storage.cl_storage();
        let path_str = self.path.to_str().unwrap();
        let cl_link = match storage
            .get_open_cl_by_path(path_str, &self.username())
            .await?
        {
            Some(cl) => cl.link.clone(),
            None => generate_link(),
        };
        let mut lock = self.cl_link.write().await;
        *lock = Some(cl_link.clone());
        Ok(cl_link)
    }

    pub fn username(&self) -> String {
        self.username.clone().unwrap_or(String::from("Anonymous"))
    }
}
