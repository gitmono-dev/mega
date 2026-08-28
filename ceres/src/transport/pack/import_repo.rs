use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use async_recursion::async_recursion;
use async_trait::async_trait;
use callisto::sea_orm_active_enums::RefTypeEnum;
use common::{errors::MegaError, utils::ZERO_ID};
use futures::{StreamExt, TryStreamExt};
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::{EntryMeta, MetaAttached},
        object::{blob::Blob, commit::Commit, tag::Tag, tree::Tree},
        pack::{encode::PackEncoder, entry::Entry},
    },
};
use io_orbit::object_storage::MultiObjectByteStream;
use jupiter::{
    redis::lock::RedLock,
    service::git_service::GitService,
    storage::{Storage, base_storage::StorageConnector, git_db_storage::GitDbStorage},
    utils::converter::FromGitModel,
};
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    bus::{ApplicationEventHandler, TransportEvent},
    infra::cache::GitObjectCache,
    transport::{
        pack::RepoHandler,
        protocol::{
            import_refs::{CommandType, RefCommand, Refs},
            repo::Repo,
        },
    },
};

pub struct ImportRepo {
    pub storage: Storage,
    pub repo: Repo,
    pub command_list: Mutex<Vec<RefCommand>>,
    pub unpack_redlock: Arc<RedLock>,
    pub git_object_cache: Arc<GitObjectCache>,
    pub application: Arc<dyn ApplicationEventHandler>,
    pub receive_pack_extra_timings_ms: Mutex<Vec<(String, u128)>>,
}

#[async_trait]
impl RepoHandler for ImportRepo {
    fn is_monorepo(&self) -> bool {
        false
    }

    fn save_entry_concurrency(&self) -> usize {
        self.storage.config().pack.save_entry_concurrency
    }

    fn receive_pack_extra_timings_ms(&self) -> Vec<(String, u128)> {
        std::mem::take(
            &mut self
                .receive_pack_extra_timings_ms
                .lock()
                .expect("receive_pack_extra_timings_ms lock poisoned"),
        )
    }

    fn sync_commands_after_unpack(&self, commands: &[RefCommand]) {
        *self
            .command_list
            .lock()
            .expect("command_list lock poisoned") = commands.to_vec();
    }

    async fn refs_with_head_hash(&self) -> (String, Vec<Refs>) {
        let result = self
            .storage
            .git_db_storage()
            .get_ref(self.repo.repo_id)
            .await
            .unwrap();
        let refs: Vec<Refs> = result.into_iter().map(|x| x.into()).collect();

        self.find_head_hash(refs)
    }

    async fn finalize_receive_pack(&self) -> Result<(), MegaError> {
        let t0 = Instant::now();
        let t_fp = Instant::now();
        self.traverses_tree_and_update_filepath().await?;
        self.receive_pack_extra_timings_ms
            .lock()
            .expect("receive_pack_extra_timings_ms lock poisoned")
            .push((
                "import_filepath_update_ms".to_string(),
                t_fp.elapsed().as_millis(),
            ));

        let t_attach = Instant::now();
        let commands = self
            .command_list
            .lock()
            .expect("command_list lock poisoned")
            .clone();
        let handler_timings = Arc::new(Mutex::new(Vec::new()));
        self.application
            .handle(TransportEvent::ImportReceivePackFinalized {
                repo_path: PathBuf::from(self.repo.repo_path.clone()),
                repo_id: self.repo.repo_id,
                commands,
                unpack_redlock: self.unpack_redlock.clone(),
                extra_timings: Arc::clone(&handler_timings),
            })
            .await?;
        self.receive_pack_extra_timings_ms
            .lock()
            .expect("receive_pack_extra_timings_ms lock poisoned")
            .extend(
                handler_timings
                    .lock()
                    .expect("handler_timings lock poisoned")
                    .drain(..),
            );
        self.receive_pack_extra_timings_ms
            .lock()
            .expect("receive_pack_extra_timings_ms lock poisoned")
            .extend([
                (
                    "import_attach_to_monorepo_parent_ms".to_string(),
                    t_attach.elapsed().as_millis(),
                ),
                (
                    "import_finalize_total_ms".to_string(),
                    t0.elapsed().as_millis(),
                ),
            ]);
        Ok(())
    }

    async fn save_entry(
        &self,
        entry_list: Vec<MetaAttached<Entry, EntryMeta>>,
    ) -> Result<(), MegaError> {
        self.storage
            .import_service
            .save_entry(self.repo.repo_id, entry_list)
            .await
    }

    async fn update_pack_id(&self, temp_pack_id: &str, pack_id: &str) -> Result<(), MegaError> {
        let storage = self.storage.git_db_storage();
        storage.update_pack_id(temp_pack_id, pack_id).await
    }

    async fn check_entry(&self, _: &Entry) -> Result<(), GitError> {
        Ok(())
    }

    async fn full_pack(&self, _: Vec<String>) -> Result<ReceiverStream<Vec<u8>>, GitError> {
        let pack_config = &self.storage.config().pack;
        let (entry_tx, entry_rx) = mpsc::channel(pack_config.channel_message_size);
        let (stream_tx, stream_rx) = mpsc::channel(pack_config.channel_message_size);

        let storage = self.storage.git_db_storage();
        let git_service = self.storage.git_service.clone();
        let total = storage.get_obj_count_by_repo_id(self.repo.repo_id).await;
        let encoder = PackEncoder::new(total, 0, stream_tx);
        encoder.encode_async(entry_rx).await?;

        let repo_id = self.repo.repo_id;
        tokio::spawn(async move {
            if let Err(e) = process_objects(repo_id, git_service, storage, entry_tx).await {
                tracing::error!(?e, "process_blobs failed");
            }
        });

        Ok(ReceiverStream::new(stream_rx))
    }

    async fn incremental_pack(
        &self,
        want: Vec<String>,
        have: Vec<String>,
    ) -> Result<ReceiverStream<Vec<u8>>, GitError> {
        let mut want_clone = want.clone();
        let pack_config = &self.storage.config().pack;
        let storage = self.storage.git_db_storage();
        let obj_num = AtomicUsize::new(0);

        let mut exist_objs = HashSet::new();

        let mut want_commits: Vec<Commit> = storage
            .get_commits_by_hashes(self.repo.repo_id, &want_clone)
            .await
            .unwrap()
            .into_iter()
            .map(Commit::from_git_model)
            .collect();
        let mut traversal_list: Vec<Commit> = want_commits.clone();

        // traverse commit's all parents to find the commit that client does not have
        while let Some(temp) = traversal_list.pop() {
            for p_commit_id in temp.parent_commit_ids {
                let p_commit_id = p_commit_id.to_string();

                if !have.contains(&p_commit_id) && !want_clone.contains(&p_commit_id) {
                    let parent: Commit = Commit::from_git_model(
                        storage
                            .get_commit_by_hash(self.repo.repo_id, &p_commit_id)
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
            .get_trees_by_hashes(self.repo.repo_id, want_tree_ids)
            .await
            .unwrap()
            .into_iter()
            .map(|m| {
                (
                    ObjectHash::from_str(&m.tree_id).unwrap(),
                    Tree::from_git_model(m),
                )
            })
            .collect();

        obj_num.fetch_add(want_commits.len(), Ordering::SeqCst);

        let have_commits = storage
            .get_commits_by_hashes(self.repo.repo_id, &have)
            .await
            .unwrap();
        let have_trees = storage
            .get_trees_by_hashes(
                self.repo.repo_id,
                have_commits.iter().map(|x| x.tree.clone()).collect(),
            )
            .await
            .unwrap();
        // traverse to get exist_objs
        for have_tree in have_trees {
            self.traverse(Tree::from_git_model(have_tree), &mut exist_objs, None)
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
            .git_db_storage()
            .get_trees_by_hashes(self.repo.repo_id, hashes)
            .await
            .unwrap()
            .into_iter()
            .map(Tree::from_git_model)
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
            .git_db_storage()
            .get_blobs_by_hashes(self.repo.repo_id, hashes)
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
                        // NOTE: We currently do not have CRC32 information available in the
                        // blob metadata returned from `git_db_storage()`. Downstream callers
                        // treat `None` as "CRC32 unknown" rather than "CRC32 invalid". Once
                        // pack index entries (or another source) expose CRC32 for these blobs,
                        // this should be populated with the actual checksum instead of `None`.
                        // TODO: Thread CRC32 from the underlying Git storage into `EntryMeta`.
                        crc32: None,
                    },
                )
            })
            .collect::<HashMap<String, EntryMeta>>();

        Ok(map)
    }

    async fn update_refs(&self, refs: &RefCommand) -> Result<(), GitError> {
        if refs.ref_type != RefTypeEnum::Tag {
            // Branch `import_refs` rows are written in the same DB transaction as monorepo attach.
            return Ok(());
        }
        let storage = self.storage.git_db_storage();
        match refs.command_type {
            CommandType::Create => {
                storage
                    .save_ref(self.repo.repo_id, refs.clone().into())
                    .await
                    .map_err(|e| GitError::CustomError(e.to_string()))?;
            }
            CommandType::Delete => {
                // The advertised old id is the client's lease; a stale request
                // must not remove a tag that moved after ref discovery.
                if !storage
                    .remove_ref_if_unchanged(
                        self.repo.repo_id,
                        &refs.ref_name,
                        &refs.old_id,
                        storage.get_connection(),
                    )
                    .await
                    .map_err(|e| GitError::CustomError(e.to_string()))?
                {
                    return Err(GitError::CustomError(format!(
                        "tag {} moved since advertisement (expected {})",
                        refs.ref_name, refs.old_id
                    )));
                }
            }
            CommandType::Update => {
                storage
                    .update_ref(self.repo.repo_id, &refs.ref_name, &refs.new_id)
                    .await
                    .map_err(|e| GitError::CustomError(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn check_commit_exist(&self, hash: &str) -> bool {
        self.storage
            .git_db_storage()
            .get_commit_by_hash(self.repo.repo_id, hash)
            .await
            .unwrap()
            .is_some()
    }

    /// A pushed tag may reference an annotated tag object, commit, tree, or
    /// blob; accept it only if that object is stored.
    async fn check_object_exist(&self, hash: &str) -> bool {
        let git_db = self.storage.git_db_storage();
        if git_db
            .get_tag_by_hash(self.repo.repo_id, hash)
            .await
            .unwrap()
            .is_some()
            || self.check_commit_exist(hash).await
        {
            return true;
        }
        if git_db
            .get_tree_by_hash(self.repo.repo_id, hash)
            .await
            .unwrap()
            .is_some()
        {
            return true;
        }
        !git_db
            .get_blobs_by_hashes(self.repo.repo_id, vec![hash.to_string()])
            .await
            .unwrap()
            .is_empty()
    }

    async fn check_default_branch(&self) -> bool {
        let storage = self.storage.git_db_storage();
        storage
            .default_branch_exist(self.repo.repo_id)
            .await
            .unwrap()
    }

    async fn traverses_tree_and_update_filepath(&self) -> Result<(), MegaError> {
        // Prefer the branch tip from this receive-pack (same as `attach_to_monorepo_parent`).
        // DB `import_refs` is not updated until the attach transaction, so reading HEAD only
        // from the DB would still see the pre-push tip during finalize.
        let from_commands = {
            let cmds = self
                .command_list
                .lock()
                .expect("command_list lock poisoned");
            cmds.iter()
                .find(|c| {
                    c.ref_type == RefTypeEnum::Branch && c.status == "ok" && c.new_id != ZERO_ID
                })
                .map(|c| c.new_id.clone())
        };
        let current_head = match from_commands {
            Some(h) => h,
            None => self.refs_with_head_hash().await.0,
        };
        let commit = Commit::from_git_model(
            self.storage
                .git_db_storage()
                .get_commit_by_hash(self.repo.repo_id, &current_head)
                .await?
                .unwrap(),
        );

        let root_tree = Tree::from_git_model(
            self.storage
                .git_db_storage()
                .get_tree_by_hash(self.repo.repo_id, &commit.tree_id.to_string())
                .await?
                .unwrap()
                .clone(),
        );
        let pairs = collect_git_blob_filepaths(
            self.storage.git_db_storage(),
            self.repo.repo_id,
            root_tree,
            PathBuf::new(),
        )
        .await?;
        self.storage
            .git_db_storage()
            .update_git_blob_filepaths(self.repo.repo_id, pairs)
            .await?;
        Ok(())
    }
}

#[async_recursion]
pub(crate) async fn collect_git_blob_filepaths(
    storage: GitDbStorage,
    repo_id: i64,
    tree: Tree,
    path: PathBuf,
) -> Result<Vec<(String, String)>, MegaError> {
    let mut pairs = Vec::new();
    for item in tree.tree_items {
        if item.is_tree() {
            let child = Tree::from_git_model(
                storage
                    .get_tree_by_hash(repo_id, &item.id.to_string())
                    .await?
                    .unwrap()
                    .clone(),
            );
            pairs.extend(
                collect_git_blob_filepaths(storage.clone(), repo_id, child, path.join(item.name))
                    .await?,
            );
        } else {
            pairs.push((
                item.id.to_string(),
                path.join(item.name).to_str().unwrap().to_string(),
            ));
        }
    }
    Ok(pairs)
}

async fn process_objects(
    repo_id: i64,
    git_service: GitService,
    storage: GitDbStorage,
    entry_tx: Sender<MetaAttached<Entry, EntryMeta>>,
) -> Result<(), MegaError> {
    let mut commit_stream = storage.get_commits_by_repo_id(repo_id).await?;

    while let Some(model) = commit_stream.next().await {
        match model {
            Ok(m) => {
                let c: Commit = Commit::from_git_model(m);
                let entry = MetaAttached {
                    inner: c.into(),
                    meta: EntryMeta::new(),
                };
                entry_tx.send(entry).await.expect("send error");
            }
            Err(err) => eprintln!("Error: {err:?}"),
        }
    }
    tracing::info!("send commits end");

    let mut tree_stream = storage.get_trees_by_repo_id(repo_id).await?;
    while let Some(model) = tree_stream.next().await {
        match model {
            Ok(m) => {
                let t: Tree = Tree::from_git_model(m);
                let entry = MetaAttached {
                    inner: t.into(),
                    meta: EntryMeta::new(),
                };
                entry_tx.send(entry).await.expect("send error");
            }
            Err(err) => eprintln!("Error: {err:?}"),
        }
    }
    tracing::info!("send trees end");

    let mut bid_stream = storage.get_blobs_by_repo_id(repo_id).await?;
    let mut bids = vec![];
    while let Some(model) = bid_stream.next().await {
        match model {
            Ok(m) => bids.push(m.blob_id),
            Err(err) => eprintln!("Error: {err:?}"),
        }
    }

    let entry_tx = entry_tx.clone();
    git_service
        .get_objects_stream(bids)
        .try_for_each_concurrent(16, |(_, stream, _)| {
            let sender_clone = entry_tx.clone();
            async move {
                let data = stream
                    .try_fold(Vec::new(), |mut acc, bytes| async move {
                        acc.extend_from_slice(&bytes);
                        Ok(acc)
                    })
                    .await?;
                let blob = Blob::from_content_bytes(data);
                sender_clone
                    .send(MetaAttached {
                        inner: blob.into(),
                        meta: EntryMeta::default(),
                    })
                    .await
                    .expect("send error");

                Ok(())
            }
        })
        .await?;

    tracing::info!("send blobs end");

    let tags = storage.get_tags_by_repo_id(repo_id).await?;
    for m in tags.into_iter() {
        let c: Tag = Tag::from_git_model(m);
        let entry = MetaAttached {
            inner: c.into(),
            meta: EntryMeta::new(),
        };
        entry_tx.send(entry).await.expect("send error");
    }
    tracing::info!("sending all object end...");
    Ok(())
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use callisto::{git_blob, git_tree};
    use git_internal::internal::object::{
        ObjectTrait,
        blob::Blob,
        tree::{Tree, TreeItem, TreeItemMode},
    };
    use jupiter::{
        sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter},
        storage::base_storage::StorageConnector,
        tests::test_storage,
        utils::converter::FromGitModel,
    };
    use tempfile::TempDir;

    use super::collect_git_blob_filepaths;

    #[test]
    pub fn test_recurse_tree() {
        let path = PathBuf::from("/third-party/crates/tokio/tokio-console");
        let ancestors: Vec<_> = path.ancestors().collect();
        for path in ancestors.into_iter() {
            println!("{path:?}");
        }
    }

    #[tokio::test]
    async fn collect_and_batch_update_nested_crate_tree() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        let repo_id = 11i64;

        let cargo = Blob::from_content("[package]\nname = \"demo\"\n");
        let lib = Blob::from_content("pub fn f() {}\n");
        let src_tree = Tree::from_tree_items(vec![TreeItem {
            mode: TreeItemMode::Blob,
            id: lib.id,
            name: "lib.rs".into(),
        }])
        .unwrap();
        let root_tree = Tree::from_tree_items(vec![
            TreeItem {
                mode: TreeItemMode::Blob,
                id: cargo.id,
                name: "Cargo.toml".into(),
            },
            TreeItem {
                mode: TreeItemMode::Tree,
                id: src_tree.id,
                name: "src".into(),
            },
        ])
        .unwrap();

        let now = chrono::Utc::now().naive_utc();
        git_blob::Entity::insert_many([
            git_blob::Model {
                id: 1,
                repo_id,
                blob_id: cargo.id.to_string(),
                name: None,
                size: 0,
                created_at: now,
                pack_id: String::new(),
                file_path: String::new(),
                pack_offset: 0,
                is_delta_in_pack: false,
            }
            .into_active_model(),
            git_blob::Model {
                id: 2,
                repo_id,
                blob_id: lib.id.to_string(),
                name: None,
                size: 0,
                created_at: now,
                pack_id: String::new(),
                file_path: String::new(),
                pack_offset: 0,
                is_delta_in_pack: false,
            }
            .into_active_model(),
        ])
        .exec(stg.get_connection())
        .await
        .unwrap();

        git_tree::Entity::insert_many([
            git_tree::Model {
                id: 3,
                repo_id,
                tree_id: src_tree.id.to_string(),
                sub_trees: src_tree.to_data().unwrap(),
                size: 0,
                created_at: now,
                pack_id: String::new(),
                pack_offset: 0,
            }
            .into_active_model(),
            git_tree::Model {
                id: 4,
                repo_id,
                tree_id: root_tree.id.to_string(),
                sub_trees: root_tree.to_data().unwrap(),
                size: 0,
                created_at: now,
                pack_id: String::new(),
                pack_offset: 0,
            }
            .into_active_model(),
        ])
        .exec(stg.get_connection())
        .await
        .unwrap();

        let loaded_root = Tree::from_git_model(
            stg.get_tree_by_hash(repo_id, &root_tree.id.to_string())
                .await
                .unwrap()
                .unwrap(),
        );
        let mut pairs =
            collect_git_blob_filepaths(stg.clone(), repo_id, loaded_root, PathBuf::new())
                .await
                .unwrap();
        pairs.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            pairs,
            vec![
                (cargo.id.to_string(), "Cargo.toml".into()),
                (lib.id.to_string(), "src/lib.rs".into()),
            ]
        );

        stg.update_git_blob_filepaths(repo_id, pairs).await.unwrap();

        let cargo_row = git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .filter(git_blob::Column::BlobId.eq(cargo.id.to_string()))
            .one(stg.get_connection())
            .await
            .unwrap()
            .unwrap();
        let lib_row = git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .filter(git_blob::Column::BlobId.eq(lib.id.to_string()))
            .one(stg.get_connection())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cargo_row.file_path, "Cargo.toml");
        assert_eq!(lib_row.file_path, "src/lib.rs");
    }
}
