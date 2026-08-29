use std::sync::Arc;

use callisto::{git_blob, git_commit, git_tag, git_tree};
use common::errors::MegaError;
use futures::{StreamExt, stream};
use git_internal::internal::{
    metadata::{EntryMeta, MetaAttached},
    pack::entry::Entry,
};
use sea_orm::IntoActiveModel;
use tokio::sync::Mutex;

use crate::{
    service::git_service::GitService,
    storage::{
        base_storage::{BaseStorage, StorageConnector},
        git_db_storage::GitDbStorage,
    },
    utils::converter::{GitObjectModel, active_hash, process_entry},
};

#[derive(Clone)]
pub struct ImportService {
    pub git_db_storage: GitDbStorage,
    pub git_service: GitService,
}

#[derive(Debug, Default)]
pub struct GitObjects {
    commits: Vec<git_commit::ActiveModel>,
    trees: Vec<git_tree::ActiveModel>,
    blobs: Vec<git_blob::ActiveModel>,
    tags: Vec<git_tag::ActiveModel>,
}

impl ImportService {
    pub fn mock() -> Self {
        let mock = BaseStorage::mock();
        let git_db_storage = GitDbStorage { base: mock.clone() };
        let git_service = GitService::mock();

        Self {
            git_db_storage,
            git_service,
        }
    }

    pub async fn save_entry(
        &self,
        repo_id: i64,
        entry_list: Vec<MetaAttached<Entry, EntryMeta>>,
    ) -> Result<(), MegaError> {
        let git_objects = Arc::new(Mutex::new(GitObjects::default()));

        let results: Vec<Result<(), MegaError>> = stream::iter(entry_list)
            .map(|entry| {
                let git_objects = git_objects.clone();
                async move {
                    let raw_obj = process_entry(entry.inner);
                    let model = raw_obj.convert_to_git_model(entry.meta);

                    match model {
                        GitObjectModel::Commit(mut commit) => {
                            commit.repo_id = repo_id;

                            git_objects
                                .lock()
                                .await
                                .commits
                                .push(commit.into_active_model())
                        }
                        GitObjectModel::Tree(mut tree) => {
                            tree.repo_id = repo_id;
                            git_objects
                                .lock()
                                .await
                                .trees
                                .push(tree.into_active_model());
                        }
                        GitObjectModel::Blob(mut blob, raw) => {
                            blob.repo_id = repo_id;

                            self.git_service
                                .save_object_from_model(raw, &blob.blob_id)
                                .await?;

                            let mut guard = git_objects.lock().await;
                            guard.blobs.push(blob.into_active_model());
                        }
                        GitObjectModel::Tag(mut tag) => {
                            tag.repo_id = repo_id;
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

        self.git_db_storage
            .batch_save_model(git_objects.commits)
            .await?;
        self.git_db_storage
            .batch_save_model(git_objects.trees)
            .await?;
        self.git_db_storage
            .batch_save_model(git_objects.blobs)
            .await?;
        self.git_db_storage
            .batch_save_model(git_objects.tags)
            .await?;
        Ok(())
    }
}
