use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use api_model::common::Pagination;
use async_trait::async_trait;
use callisto::{git_tag, import_refs};
use common::errors::MegaError;
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::{EntryMeta, MetaAttached},
        object::{
            commit::Commit,
            tree::{Tree, TreeItem},
        },
        pack::entry::Entry,
    },
};
use jupiter::{storage::Storage, utils::converter::FromGitModel};

use crate::{
    application::api_service::{
        ApiHandler,
        cache::GitObjectCache,
        history,
        mono::MonoServiceLogic,
        tag_ops::{
            self, build_git_internal_tag, db_error, format_tagger_info, is_annotated_tag,
            lightweight_commit_tag, merge_paginated_tags, tag_already_exists, tags_full_ref,
        },
    },
    model::{
        git::{CreateEntryInfo, CreateEntryResult, EditFilePayload, EditFileResult},
        tag::TagInfo,
    },
    transport::protocol::repo::Repo,
};

#[derive(Clone)]
pub struct ImportApiService {
    storage: Storage,
    repo: Repo,
    git_object_cache: Arc<GitObjectCache>,
}

impl ImportApiService {
    pub fn new(storage: Storage, repo: Repo, git_object_cache: Arc<GitObjectCache>) -> Self {
        Self {
            storage,
            repo,
            git_object_cache,
        }
    }
}

#[async_trait]
impl ApiHandler for ImportApiService {
    fn get_context(&self) -> Storage {
        self.storage.clone()
    }

    fn object_cache(&self) -> &GitObjectCache {
        &self.git_object_cache
    }

    async fn create_monorepo_entry(
        &self,
        _: CreateEntryInfo,
    ) -> Result<CreateEntryResult, GitError> {
        Err(GitError::CustomError(
            "import dir does not support create entry".to_string(),
        ))
    }

    fn strip_relative(&self, path: &Path) -> Result<PathBuf, MegaError> {
        let path_str = path.to_string_lossy();

        // If path is truly relative (no leading slash and not absolute), return as-is
        if !path_str.starts_with('/') && !path.is_absolute() {
            // Check if it doesn't start with repo_path either
            let repo_trimmed = self.repo.repo_path.trim_start_matches('/');
            if !path_str.starts_with(repo_trimmed) {
                tracing::debug!("strip_relative -> path is already relative: {:?}", path);
                return Ok(path.to_path_buf());
            }
        }

        // Normalize both paths by removing leading slashes for consistent comparison
        let path_trimmed = path_str.trim_start_matches('/');
        let repo_trimmed = self.repo.repo_path.trim_start_matches('/');

        let path_normalized = Path::new(path_trimmed);
        let repo_normalized = Path::new(repo_trimmed);

        match path_normalized.strip_prefix(repo_normalized) {
            Ok(relative_path) => {
                tracing::debug!("strip_relative -> relative={:?}", relative_path);
                Ok(relative_path.to_path_buf())
            }
            Err(_) => Err(MegaError::Other(format!(
                "Path '{}' is not under repo '{}'",
                path.display(),
                self.repo.repo_path
            ))),
        }
    }

    async fn get_root_commit(&self) -> Result<Commit, MegaError> {
        let storage = self.storage.git_db_storage();
        let source = crate::application::snapshot::source::resolve_import_commit(
            &storage,
            self.repo.repo_id,
            None,
        )
        .await?;
        self.get_commit_by_hash(&source.commit_oid).await
    }

    /// Resolve once, then read that immutable root. An explicit revision must
    /// never silently fall back to the repository's current default branch.
    async fn get_root_tree(&self, refs: Option<&str>) -> Result<Tree, MegaError> {
        let storage = self.storage.git_db_storage();
        let source = crate::application::snapshot::source::resolve_import_commit(
            &storage,
            self.repo.repo_id,
            refs,
        )
        .await?;
        crate::application::snapshot::source::read_import_root(&storage, &source).await
    }

    async fn get_tree_by_hash(&self, hash: &str) -> Result<Tree, MegaError> {
        let model = self
            .storage
            .git_db_storage()
            .get_tree_by_hash(self.repo.repo_id, hash)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("import tree not found: {hash}")))?;
        Ok(Tree::from_git_model(model))
    }

    async fn get_commit_by_hash(&self, hash: &str) -> Result<Commit, MegaError> {
        let storage = self.storage.git_db_storage();
        let commit = storage
            .get_commit_by_hash(self.repo.repo_id, hash)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("import commit not found: {hash}")))?;
        Ok(Commit::from_git_model(commit))
    }

    async fn get_commits_by_hashes(&self, c_hashes: Vec<String>) -> Result<Vec<Commit>, GitError> {
        let storage = self.storage.git_db_storage();
        let commits = storage
            .get_commits_by_hashes(self.repo.repo_id, &c_hashes)
            .await
            .unwrap();
        Ok(commits.into_iter().map(Commit::from_git_model).collect())
    }

    async fn item_to_commit_map(
        &self,
        path: PathBuf,
        reference: Option<&str>,
    ) -> Result<HashMap<TreeItem, Option<Commit>>, GitError> {
        history::item_to_commit_map(self, path, reference).await
    }

    async fn create_tag(
        &self,
        _repo_path: Option<String>,
        name: String,
        target: Option<String>,
        tagger_name: Option<String>,
        tagger_email: Option<String>,
        message: Option<String>,
    ) -> Result<TagInfo, GitError> {
        let git_storage = self.storage.git_db_storage();
        let tagger_info = format_tagger_info(tagger_name, tagger_email);

        self.validate_target_commit(target.as_ref()).await?;

        let full_ref = tags_full_ref(&name);
        match git_storage
            .get_tag_by_repo_and_name(self.repo.repo_id, &name)
            .await
        {
            Ok(Some(_)) => return Err(tag_already_exists(&name).into()),
            Ok(None) => {}
            Err(e) => {
                tracing::error!("DB error while checking git_tag existence: {}", e);
                return Err(db_error().into());
            }
        }

        if let Ok(refs) = git_storage.get_ref(self.repo.repo_id).await
            && refs.iter().any(|r| r.ref_name == full_ref)
        {
            return Err(tag_already_exists(&name).into());
        }

        if is_annotated_tag(&message) {
            return self
                .create_annotated_tag(&git_storage, full_ref, name, target, tagger_info, message)
                .await;
        }

        self.create_lightweight_tag(&git_storage, full_ref, name, target, tagger_info)
            .await
    }

    async fn list_tags(
        &self,
        _repo_path: Option<String>,
        pagination: Pagination,
    ) -> Result<(Vec<TagInfo>, u64), GitError> {
        let git_storage = self.storage.git_db_storage();
        // annotated tags: fetch paged annotated tags from storage
        let (annotated_tags_page, annotated_total) = match git_storage
            .list_tags_by_repo_with_page(self.repo.repo_id, pagination.clone())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("DB error while listing git tags: {}", e);
                return Err(GitError::CustomError("[code:500] DB error".to_string()));
            }
        };

        let result: Vec<TagInfo> = annotated_tags_page
            .into_iter()
            .map(|t| TagInfo {
                name: t.tag_name,
                tag_id: t.tag_id,
                object_id: t.object_id,
                object_type: t.object_type,
                tagger: t.tagger,
                message: t.message,
                created_at: t.created_at.and_utc().to_rfc3339(),
            })
            .collect();

        let mut lightweight_refs: Vec<TagInfo> = vec![];
        if let Ok(refs) = git_storage.get_ref(self.repo.repo_id).await {
            for r in refs {
                if r.ref_name.starts_with("refs/tags/") {
                    let tag_name = r.ref_name.trim_start_matches("refs/tags/").to_string();
                    if result.iter().any(|t| t.name == tag_name) {
                        continue;
                    }
                    lightweight_refs.push(lightweight_commit_tag(
                        tag_name,
                        r.ref_git_id.clone(),
                        "",
                        r.created_at.and_utc().to_rfc3339(),
                    ));
                }
            }
        }

        Ok(merge_paginated_tags(
            result,
            lightweight_refs,
            annotated_total,
            pagination.per_page,
        ))
    }

    async fn get_tag(
        &self,
        _repo_path: Option<String>,
        name: String,
    ) -> Result<Option<TagInfo>, GitError> {
        let git_storage = self.storage.git_db_storage();
        // annotated first: use jupiter git_storage helper
        match git_storage
            .get_tag_by_repo_and_name(self.repo.repo_id, &name)
            .await
        {
            Ok(Some(tag)) => {
                return Ok(Some(TagInfo {
                    name: tag.tag_name,
                    tag_id: tag.tag_id,
                    object_id: tag.object_id,
                    object_type: tag.object_type,
                    tagger: tag.tagger,
                    message: tag.message,
                    created_at: tag.created_at.and_utc().to_rfc3339(),
                }));
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!("DB error while getting git tag: {}", e);
                return Err(GitError::CustomError("[code:500] DB error".to_string()));
            }
        }
        let full_ref = tags_full_ref(&name);
        if let Ok(refs) = git_storage.get_ref(self.repo.repo_id).await {
            for r in refs {
                if r.ref_name == full_ref {
                    return Ok(Some(lightweight_commit_tag(
                        name,
                        r.ref_git_id.clone(),
                        "",
                        r.created_at.and_utc().to_rfc3339(),
                    )));
                }
            }
        }
        Ok(None)
    }

    async fn delete_tag(&self, _repo_path: Option<String>, name: String) -> Result<(), GitError> {
        let git_storage = self.storage.git_db_storage();
        // annotated first: use jupiter helpers
        match git_storage
            .get_tag_by_repo_and_name(self.repo.repo_id, &name)
            .await
        {
            Ok(Some(_tag)) => {
                let full_ref = tags_full_ref(&name);
                git_storage
                    .remove_ref(self.repo.repo_id, &full_ref)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            "Failed to remove import ref when deleting annotated tag: {}",
                            e
                        );
                        GitError::CustomError("[code:500] Failed to remove import ref".to_string())
                    })?;
                git_storage
                    .delete_tag(self.repo.repo_id, &name)
                    .await
                    .map_err(|e| {
                        tracing::error!("DB delete error when deleting annotated git tag: {}", e);
                        GitError::CustomError("[code:500] DB delete error".to_string())
                    })?;
                Ok(())
            }
            Ok(None) => {
                let full_ref = tags_full_ref(&name);
                git_storage
                    .remove_ref(self.repo.repo_id, &full_ref)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            "Failed to remove import ref when deleting lightweight tag: {}",
                            e
                        );
                        GitError::CustomError("[code:500] Failed to remove import ref".to_string())
                    })?;
                Ok(())
            }
            Err(e) => Err(GitError::CustomError(format!("[code:500] DB error: {}", e))),
        }
    }

    /// Save file edit for import repo path
    async fn save_file_edit(&self, payload: EditFilePayload) -> Result<EditFileResult, GitError> {
        use git_internal::internal::object::{blob::Blob, tree::TreeItemMode};

        let path = PathBuf::from(&payload.path);
        let parent = path
            .parent()
            .ok_or_else(|| GitError::CustomError("Invalid file path".to_string()))?;
        let update_chain = self
            .search_tree_for_update(parent)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;
        let parent_tree = update_chain
            .last()
            .cloned()
            .ok_or_else(|| GitError::CustomError("Parent tree not found".to_string()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| GitError::CustomError("Invalid file name".to_string()))?;
        let _current = parent_tree
            .tree_items
            .iter()
            .find(|x| x.name == name && x.mode == TreeItemMode::Blob)
            .ok_or_else(|| GitError::CustomError("[code:404] File not found".to_string()))?;

        // Create new blob and rebuild tree up to root
        let new_blob = Blob::from_content(&payload.content);
        let (updated_trees, new_root_id) =
            MonoServiceLogic::propagate_tree_chain(path.clone(), update_chain, new_blob.id)?;

        // Save commit and objects under import repo tables
        let git_storage = self.storage.git_db_storage();
        let new_commit_id = {
            // Update default branch ref commit with parent = current default commit
            let default_ref = git_storage
                .get_default_ref(self.repo.repo_id)
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?
                .ok_or_else(|| GitError::CustomError("Default ref not found".to_string()))?;
            let current_commit = git_storage
                .get_commit_by_hash(self.repo.repo_id, &default_ref.ref_git_id)
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?
                .ok_or(GitError::InvalidCommitObject)?;
            let parent_id = ObjectHash::from_str(&current_commit.commit_id).unwrap();

            let new_commit =
                Commit::from_tree_id(new_root_id, vec![parent_id], &payload.commit_message);
            let new_commit_id = new_commit.id.to_string();

            let mut entries: Vec<MetaAttached<Entry, EntryMeta>> = Vec::new();
            for t in updated_trees.iter().cloned() {
                entries.push(MetaAttached {
                    inner: Entry::from(t),
                    meta: EntryMeta::new(),
                });
            }
            entries.push(MetaAttached {
                inner: Entry::from(new_blob.clone()),
                meta: EntryMeta::new(),
            });
            entries.push(MetaAttached {
                inner: Entry::from(new_commit.clone()),
                meta: EntryMeta::new(),
            });
            self.storage
                .import_service
                .save_entry(self.repo.repo_id, entries)
                .await?;

            // Update ref to new commit id
            git_storage
                .update_ref(self.repo.repo_id, &default_ref.ref_name, &new_commit_id)
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?;
            new_commit_id
        };

        Ok(EditFileResult {
            commit_id: new_commit_id,
            new_oid: new_blob.id.to_string(),
            path: payload.path,
            cl_link: None,
        })
    }
}

impl ImportApiService {
    async fn create_annotated_tag(
        &self,
        git_storage: &jupiter::storage::git_db_storage::GitDbStorage,
        full_ref: String,
        name: String,
        target: Option<String>,
        tagger_info: String,
        message: Option<String>,
    ) -> Result<TagInfo, GitError> {
        // build git_internal tag and models
        let (tag_id_hex, object_id) =
            build_git_internal_tag(name.clone(), target, tagger_info.clone(), message.clone())?;

        let new_model = self.build_git_tag_model(
            tag_id_hex.clone(),
            object_id.clone(),
            name.clone(),
            tagger_info.clone(),
            message.clone(),
        );
        match git_storage.insert_tag(new_model).await {
            Ok(saved) => {
                // write import ref; rollback handled inside helper
                self.write_import_ref_with_rollback(
                    full_ref.clone(),
                    object_id.clone(),
                    self.repo.repo_id,
                    &name,
                )
                .await?;
                Ok(TagInfo {
                    name: saved.tag_name,
                    tag_id: saved.tag_id,
                    object_id: saved.object_id,
                    object_type: saved.object_type,
                    tagger: saved.tagger,
                    message: saved.message,
                    created_at: saved.created_at.and_utc().to_rfc3339(),
                })
            }
            Err(e) => {
                tracing::error!("DB insert error when creating annotated git tag: {}", e);
                Err(GitError::CustomError(
                    "[code:500] DB insert error".to_string(),
                ))
            }
        }
    }

    async fn create_lightweight_tag(
        &self,
        git_storage: &jupiter::storage::git_db_storage::GitDbStorage,
        full_ref: String,
        name: String,
        target: Option<String>,
        tagger_info: String,
    ) -> Result<TagInfo, GitError> {
        let object_id = target.clone().unwrap_or_default();
        let import_ref = import_refs::Model {
            id: common::utils::generate_id(),
            repo_id: self.repo.repo_id,
            ref_name: full_ref.clone(),
            ref_git_id: object_id.clone(),
            ref_type: callisto::sea_orm_active_enums::RefTypeEnum::Tag,
            default_branch: false,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        // Use ref creation time as lightweight tag created_at (capture before move)
        let created_at = import_ref.created_at.and_utc().to_rfc3339();
        git_storage
            .save_ref(self.repo.repo_id, import_ref)
            .await
            .map_err(|e| {
                tracing::error!("Failed to write import ref for lightweight tag: {}", e);
                GitError::CustomError("[code:500] Failed to write import ref".to_string())
            })?;
        Ok(lightweight_commit_tag(
            name,
            object_id,
            tagger_info,
            created_at,
        ))
    }

    async fn validate_target_commit(&self, target: Option<&String>) -> Result<(), GitError> {
        if let Some(t) = target {
            let git_storage = self.storage.git_db_storage();
            match git_storage.get_commit_by_hash(self.repo.repo_id, t).await {
                Ok(c) => {
                    if c.is_none() {
                        return Err(tag_ops::commit_not_found(t).into());
                    }
                }
                Err(e) => {
                    tracing::error!("DB error while fetching commit by hash: {}", e);
                    return Err(db_error().into());
                }
            }
        }
        Ok(())
    }

    fn build_git_tag_model(
        &self,
        tag_id_hex: String,
        object_id: String,
        name: String,
        tagger_info: String,
        message: Option<String>,
    ) -> git_tag::Model {
        git_tag::Model {
            id: common::utils::generate_id(),
            repo_id: self.repo.repo_id,
            tag_id: tag_id_hex,
            object_id,
            object_type: "commit".to_string(),
            tag_name: name,
            tagger: tagger_info,
            message: message.unwrap_or_default(),
            pack_id: String::new(),
            pack_offset: 0,

            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    async fn write_import_ref_with_rollback(
        &self,
        full_ref: String,
        object_id: String,
        repo_id: i64,
        tag_name: &str,
    ) -> Result<(), GitError> {
        let git_storage = self.storage.git_db_storage();
        let import_ref = import_refs::Model {
            id: common::utils::generate_id(),
            repo_id,
            ref_name: full_ref.clone(),
            ref_git_id: object_id.clone(),
            ref_type: callisto::sea_orm_active_enums::RefTypeEnum::Tag,
            default_branch: false,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        if let Err(e) = git_storage.save_ref(repo_id, import_ref).await {
            if let Err(del_e) = git_storage.delete_tag(repo_id, tag_name).await {
                tracing::error!(
                    "Failed to rollback git_tag DB record after ref write failure: {}",
                    del_e
                );
            }
            tracing::error!("Failed to write import ref after DB insert: {}", e);
            return Err(GitError::CustomError(
                "[code:500] Failed to write import ref".to_string(),
            ));
        }
        Ok(())
    }
}
