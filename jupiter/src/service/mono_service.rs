use std::sync::Arc;

use callisto::{mega_blob, mega_commit, mega_tag, mega_tree};
use common::{config::MonoConfig, errors::MegaError};
use futures::{StreamExt, stream};
use git_internal::internal::{
    metadata::{EntryMeta, MetaAttached},
    object::blob::Blob,
    pack::entry::Entry,
};
use sea_orm::{IntoActiveModel, TransactionTrait};
use tokio::sync::Mutex;

use crate::{
    service::git_service::GitService,
    storage::{
        base_storage::{BaseStorage, StorageConnector},
        mono_storage::MonoStorage,
    },
    utils::converter::{
        IntoMegaModel, MegaModelConverter, MegaObjectModel, active_hash, process_entry,
    },
};

#[derive(Clone)]
pub struct MonoService {
    pub mono_storage: MonoStorage,
    pub git_service: GitService,
}

#[derive(Debug, Default)]
pub struct GitMegaObjects {
    commits: Vec<mega_commit::ActiveModel>,
    trees: Vec<mega_tree::ActiveModel>,
    blobs: Vec<mega_blob::ActiveModel>,
    tags: Vec<mega_tag::ActiveModel>,
}

impl MonoService {
    pub fn mock() -> Self {
        let mock = BaseStorage::mock();
        let mono_storage = MonoStorage { base: mock.clone() };
        let git_service = GitService::mock();

        Self {
            mono_storage,
            git_service,
        }
    }

    pub async fn init_monorepo(&self, mono_config: &MonoConfig) -> Result<(), MegaError> {
        if self.mono_storage.get_main_ref("/").await?.is_some() {
            tracing::info!("Monorepo Directory Already Inited, skip init process!");
            return Ok(());
        }
        let txn = self.mono_storage.get_connection().begin().await?;
        let converter = MegaModelConverter::init(mono_config);
        let commit = converter
            .commit
            .into_mega_model(EntryMeta::default())
            .into_active_model();

        self.mono_storage
            .batch_save_model_with_txn(vec![commit], Some(&txn))
            .await?;
        self.mono_storage
            .batch_save_model_with_txn(vec![converter.refs], Some(&txn))
            .await?;

        let mega_trees = converter.mega_trees.borrow().values().cloned().collect();
        self.mono_storage
            .batch_save_model_with_txn(mega_trees, Some(&txn))
            .await?;
        let mega_blobs = converter.mega_blobs.borrow().values().cloned().collect();
        let raw_blobs = converter.raw_blobs.into_inner();
        self.git_service.put_objects(raw_blobs).await?;
        self.mono_storage
            .batch_save_model_with_txn(mega_blobs, Some(&txn))
            .await?;
        Ok(txn.commit().await?)
    }

    pub async fn save_entry(
        &self,
        commit_id: &str,
        entry_list: Vec<MetaAttached<Entry, EntryMeta>>,
    ) -> Result<Vec<mega_commit::ActiveModel>, MegaError> {
        let git_objects = Arc::new(Mutex::new(GitMegaObjects::default()));

        let results: Vec<Result<(), MegaError>> = stream::iter(entry_list)
            .map(|entry| {
                let git_objects = git_objects.clone();
                async move {
                    let raw_obj = process_entry(entry.inner);

                    let model = raw_obj.convert_to_mega_model(entry.meta);
                    match model {
                        MegaObjectModel::Commit(commit) => git_objects
                            .lock()
                            .await
                            .commits
                            .push(commit.into_active_model()),
                        MegaObjectModel::Tree(mut tree) => {
                            commit_id.clone_into(&mut tree.commit_id);
                            git_objects
                                .lock()
                                .await
                                .trees
                                .push(tree.into_active_model());
                        }
                        MegaObjectModel::Blob(mut blob, raw) => {
                            commit_id.clone_into(&mut blob.commit_id);

                            self.git_service
                                .save_object_from_model(raw, &blob.blob_id)
                                .await?;
                            git_objects
                                .lock()
                                .await
                                .blobs
                                .push(blob.into_active_model());
                        }
                        MegaObjectModel::Tag(tag) => {
                            git_objects.lock().await.tags.push(tag.into_active_model())
                        }
                    }
                    Ok(())
                }
            })
            .buffer_unordered(16)
            .collect()
            .await;

        if let Some(err) = results.into_iter().find_map(Result::err) {
            tracing::error!("at least one blob upload to object storage failed");
            return Err(err);
        }

        let mut git_objects = Arc::try_unwrap(git_objects)
            .expect("Failed to unwrap Arc")
            .into_inner();

        git_objects.blobs.sort_by_key(|a| active_hash(&a.blob_id));
        git_objects.trees.sort_by_key(|a| active_hash(&a.tree_id));
        git_objects
            .commits
            .sort_by_key(|a| active_hash(&a.commit_id));

        self.mono_storage
            .batch_save_model(git_objects.commits.clone())
            .await?;
        self.mono_storage
            .batch_save_model(git_objects.trees)
            .await?;
        self.mono_storage
            .batch_save_model(git_objects.blobs)
            .await?;
        self.mono_storage.batch_save_model(git_objects.tags).await?;

        Ok(git_objects.commits)
    }

    /// Writes blobs to object storage (S3) first, then to DB.
    /// This order avoids leaving blob rows in DB when S3 write fails (same as save_entry).
    pub async fn save_blobs(&self, commit_id: &str, blobs: Vec<Blob>) -> Result<(), MegaError> {
        let mega_blobs: Vec<mega_blob::ActiveModel> = blobs
            .iter()
            .map(|b| (*b).clone().into_mega_model(EntryMeta::default()))
            .map(|mut m: mega_blob::Model| {
                m.commit_id = commit_id.to_owned();
                m.into_active_model()
            })
            .collect();
        self.git_service.put_objects(blobs).await?;
        self.mono_storage.batch_save_model(mega_blobs).await
    }
}
