use std::{collections::HashMap, ops::Deref};

use api_model::common::Pagination;
use callisto::{
    git_blob, git_commit, git_repo, git_tag, git_tree, import_refs,
    sea_orm_active_enums::RefTypeEnum,
};
use common::{
    errors::MegaError,
    utils::{generate_id, nested_import_repo_conflict_message},
};
use futures::Stream;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbErr, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
    sea_query::{CaseStatement, Expr, ExprTrait, OnConflict},
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone)]
pub struct GitDbStorage {
    pub base: BaseStorage,
}

impl Deref for GitDbStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl GitDbStorage {
    pub async fn create_repo_and_save_ref(
        &self,
        repo_path: &str,
        repo_name: &str,
        ref_name: &str,
        ref_id: &str,
    ) -> Result<(), MegaError> {
        // Make import repo creation idempotent:
        // - If repo_path already exists, reuse its repo_id
        // - Otherwise, create a new repo row
        let repo_id = if let Some(existing) = self.find_git_repo_exact_match(repo_path).await? {
            existing.id
        } else {
            if let Some(conflict) = self.find_nested_import_repo_conflict(repo_path).await? {
                return Err(MegaError::Conflict(nested_import_repo_conflict_message(
                    repo_path,
                    &conflict.repo_path,
                )));
            }
            let repo = git_repo::Model {
                id: generate_id(),
                repo_path: repo_path.to_string(),
                repo_name: repo_name.to_string(),
                created_at: chrono::Utc::now().naive_utc(),
                updated_at: chrono::Utc::now().naive_utc(),
            };
            self.save_git_repo(repo).await?.id
        };

        let refs = import_refs::Model {
            id: generate_id(),
            repo_id: 0,
            ref_name: ref_name.to_string(),
            ref_git_id: ref_id.to_string(),
            ref_type: RefTypeEnum::Branch,
            default_branch: true,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };
        // If ref exists, update it; otherwise insert.
        let existing_ref = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .one(self.get_connection())
            .await?;
        if existing_ref.is_some() {
            self.update_ref(repo_id, ref_name, ref_id).await?;
        } else {
            self.save_ref(repo_id, refs).await?;
        }
        Ok(())
    }

    pub async fn save_ref(
        &self,
        repo_id: i64,
        mut refs: import_refs::Model,
    ) -> Result<(), MegaError> {
        refs.repo_id = repo_id;
        let a_model = refs.into_active_model();
        import_refs::Entity::insert(a_model)
            .exec(self.get_connection())
            .await
            .map_err(|e| MegaError::Other(format!("Failed to insert import_refs: {e}")))?;
        Ok(())
    }

    pub async fn remove_ref(&self, repo_id: i64, ref_name: &str) -> Result<(), MegaError> {
        import_refs::Entity::delete_many()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn get_ref(&self, repo_id: i64) -> Result<Vec<import_refs::Model>, MegaError> {
        let result = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .order_by_asc(import_refs::Column::RefName)
            .all(self.get_connection())
            .await?;
        Ok(result)
    }

    pub async fn update_ref(
        &self,
        repo_id: i64,
        ref_name: &str,
        new_id: &str,
    ) -> Result<(), MegaError> {
        let ref_data: import_refs::Model = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .one(self.get_connection())
            .await
            .unwrap()
            .unwrap();
        let mut ref_data: import_refs::ActiveModel = ref_data.into();
        ref_data.ref_git_id = Set(new_id.to_string());
        ref_data.updated_at = Set(chrono::Utc::now().naive_utc());
        ref_data.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn save_ref_in_txn(
        &self,
        repo_id: i64,
        mut refs: import_refs::Model,
        txn: &DatabaseTransaction,
    ) -> Result<(), MegaError> {
        refs.repo_id = repo_id;
        import_refs::Entity::insert(refs.into_active_model())
            .exec(txn)
            .await
            .map_err(|e| MegaError::Other(format!("Failed to insert import_refs: {e}")))?;
        Ok(())
    }

    pub async fn remove_ref_in_txn(
        &self,
        repo_id: i64,
        ref_name: &str,
        txn: &DatabaseTransaction,
    ) -> Result<(), MegaError> {
        import_refs::Entity::delete_many()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .exec(txn)
            .await?;
        Ok(())
    }

    /// Deletes the ref only if it still points at `expected_git_id`, reporting
    /// whether a row was removed. Enforces receive-pack's advertised-old-id
    /// lease atomically: a concurrent push that moved the ref leaves it intact.
    pub async fn remove_ref_if_unchanged<C: ConnectionTrait>(
        &self,
        repo_id: i64,
        ref_name: &str,
        expected_git_id: &str,
        conn: &C,
    ) -> Result<bool, MegaError> {
        let result = import_refs::Entity::delete_many()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .filter(import_refs::Column::RefGitId.eq(expected_git_id))
            .exec(conn)
            .await?;
        Ok(result.rows_affected > 0)
    }

    pub async fn update_ref_in_txn(
        &self,
        repo_id: i64,
        ref_name: &str,
        new_id: &str,
        txn: &DatabaseTransaction,
    ) -> Result<(), MegaError> {
        let ref_data = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::RefName.eq(ref_name))
            .one(txn)
            .await?
            .ok_or_else(|| MegaError::Other(format!("import_refs not found: {ref_name}")))?;
        let mut active: import_refs::ActiveModel = ref_data.into();
        active.ref_git_id = Set(new_id.to_string());
        active.updated_at = Set(chrono::Utc::now().naive_utc());
        active.update(txn).await?;
        Ok(())
    }

    pub async fn get_default_ref(
        &self,
        repo_id: i64,
    ) -> Result<Option<import_refs::Model>, MegaError> {
        let result = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::DefaultBranch.eq(true))
            .one(self.get_connection())
            .await?;
        Ok(result)
    }

    pub async fn default_branch_exist(&self, repo_id: i64) -> Result<bool, MegaError> {
        let result = import_refs::Entity::find()
            .filter(import_refs::Column::RepoId.eq(repo_id))
            .filter(import_refs::Column::DefaultBranch.eq(true))
            .count(self.get_connection())
            .await?;
        Ok(result > 0)
    }

    pub async fn update_pack_id(&self, temp_pack_id: &str, pack_id: &str) -> Result<(), MegaError> {
        let conn = self.get_connection();

        //
        let txn: DatabaseTransaction = conn.begin().await?;

        //
        let tables = [
            (
                "git_blob",
                git_blob::Entity::update_many()
                    .col_expr(git_blob::Column::PackId, Expr::value(pack_id))
                    .filter(git_blob::Column::PackId.eq(temp_pack_id))
                    .exec(&txn)
                    .await?,
            ),
            (
                "git_tree",
                git_tree::Entity::update_many()
                    .col_expr(git_tree::Column::PackId, Expr::value(pack_id))
                    .filter(git_tree::Column::PackId.eq(temp_pack_id))
                    .exec(&txn)
                    .await?,
            ),
            (
                "git_tag",
                git_tag::Entity::update_many()
                    .col_expr(git_tag::Column::PackId, Expr::value(pack_id))
                    .filter(git_tag::Column::PackId.eq(temp_pack_id))
                    .exec(&txn)
                    .await?,
            ),
            (
                "git_commit",
                git_commit::Entity::update_many()
                    .col_expr(git_commit::Column::PackId, Expr::value(pack_id))
                    .filter(git_commit::Column::PackId.eq(temp_pack_id))
                    .exec(&txn)
                    .await?,
            ),
        ];

        //
        for (name, res) in tables {
            if res.rows_affected > 0 {
                tracing::info!(" git object Updated {} rows in {}", res.rows_affected, name);
            }
        }

        //
        txn.commit().await?;
        Ok(())
    }

    pub async fn update_git_blob_filepath(
        &self,
        repo_id: i64,
        blob_id: &str,
        file_path: &str,
    ) -> Result<(), MegaError> {
        self.update_git_blob_filepaths(repo_id, vec![(blob_id.to_string(), file_path.to_string())])
            .await
    }

    /// Batch-assign `file_path` for blobs in one repo.
    ///
    /// Duplicate `blob_id`s keep the last path (same as sequential UPDATE). Missing ids are
    /// skipped. Empty input is a no-op.
    pub async fn update_git_blob_filepaths(
        &self,
        repo_id: i64,
        pairs: Vec<(String, String)>,
    ) -> Result<(), MegaError> {
        if pairs.is_empty() {
            return Ok(());
        }

        let collapsed = last_wins_filepaths(pairs);
        for chunk in collapsed.chunks(<BaseStorage as StorageConnector>::BATCH_CHUNK_SIZE) {
            let blob_ids: Vec<String> = chunk.iter().map(|(id, _)| id.clone()).collect();
            let mut case = CaseStatement::new();
            for (blob_id, file_path) in chunk {
                case = case.case(
                    Expr::col(git_blob::Column::BlobId).eq(blob_id.clone()),
                    file_path.clone(),
                );
            }
            case = case.finally(Expr::col(git_blob::Column::FilePath));

            git_blob::Entity::update_many()
                .col_expr(git_blob::Column::FilePath, case.into())
                .filter(git_blob::Column::RepoId.eq(repo_id))
                .filter(git_blob::Column::BlobId.is_in(blob_ids))
                .exec(self.get_connection())
                .await?;
        }
        Ok(())
    }

    /// Finds a Git repository with an exact match on the repository path.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - A string slice that holds the path of the repository to search for.
    ///
    /// # Returns
    ///
    /// A `Result` containing an `Option` with the Git repository model if found, or `None` if not found.
    /// Returns a `MegaError` if an error occurs during the search.
    pub async fn find_git_repo_exact_match(
        &self,
        repo_path: &str,
    ) -> Result<Option<git_repo::Model>, MegaError> {
        let result = git_repo::Entity::find()
            .filter(git_repo::Column::RepoPath.eq(repo_path))
            .one(self.get_connection())
            .await?;
        Ok(result)
    }

    /// Returns an existing import repo that would nest with `repo_path` if a new
    /// import repo were created there (ancestor or descendant by path segment).
    ///
    /// Same-path repos are not conflicts (caller should use exact match first).
    pub async fn find_nested_import_repo_conflict(
        &self,
        repo_path: &str,
    ) -> Result<Option<git_repo::Model>, MegaError> {
        let path = repo_path.trim_end_matches('/');
        if path.is_empty() || path == "/" {
            return Ok(None);
        }

        // Descendant of the new path (new path would become a parent import repo).
        // Btree range `[path/, path0)` uses idx_ir_repo_path; `LIKE 'path/%'` does not
        // (needs varchar_pattern_ops / C collation) and seq-scans ~1M rows.
        let (lo, hi) = git_repo_descendant_bounds(path);
        if let Some(descendant) = git_repo::Entity::find()
            .filter(git_repo::Column::RepoPath.gte(lo))
            .filter(git_repo::Column::RepoPath.lt(hi))
            .one(self.get_connection())
            .await?
        {
            return Ok(Some(descendant));
        }

        // Ancestor of the new path (new path would nest under an existing import repo).
        let mut current = std::path::PathBuf::from(path);
        while current.pop() {
            let parent = current.to_string_lossy();
            let parent = if parent.is_empty() {
                "/".to_string()
            } else {
                parent.to_string()
            };
            if parent == "/" {
                break;
            }
            if let Some(ancestor) = self.find_git_repo_exact_match(&parent).await? {
                return Ok(Some(ancestor));
            }
        }

        Ok(None)
    }

    /// Longest import repo whose path is a segment-prefix of `repo_path`.
    ///
    /// Walks parents with the unique `repo_path` index instead of
    /// `'{path}' LIKE repo_path || '%'` (seq scan, and a false match on
    /// `/third-party/rust` vs `/third-party/rust_v1`).
    pub async fn find_git_repo_like_path(
        &self,
        repo_path: &str,
    ) -> Result<Option<git_repo::Model>, MegaError> {
        let path = repo_path.trim_end_matches('/');
        if path.is_empty() {
            return Ok(None);
        }

        let mut current = std::path::PathBuf::from(path);
        loop {
            let candidate = current.to_string_lossy();
            let candidate = if candidate.is_empty() {
                "/".to_string()
            } else {
                candidate.to_string()
            };
            if let Some(repo) = self.find_git_repo_exact_match(&candidate).await? {
                return Ok(Some(repo));
            }
            if candidate == "/" || !current.pop() {
                break;
            }
        }
        Ok(None)
    }

    pub async fn save_git_repo(&self, repo: git_repo::Model) -> Result<git_repo::Model, MegaError> {
        let repo_path = repo.repo_path.clone();
        let insert = git_repo::Entity::insert(repo.into_active_model()).on_conflict(
            OnConflict::column(git_repo::Column::RepoPath)
                .do_nothing()
                .to_owned(),
        );
        match insert.exec(self.get_connection()).await {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(e) => {
                return Err(MegaError::Other(format!("Failed to insert git_repo: {e}")));
            }
        }
        self.find_git_repo_exact_match(&repo_path)
            .await?
            .ok_or_else(|| MegaError::Other(format!("git_repo missing after save: {repo_path}")))
    }

    pub async fn get_commit_by_hash(
        &self,
        repo_id: i64,
        hash: &str,
    ) -> Result<Option<git_commit::Model>, MegaError> {
        Ok(git_commit::Entity::find()
            .filter(git_commit::Column::RepoId.eq(repo_id))
            .filter(git_commit::Column::CommitId.eq(hash))
            .one(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_commits_by_hashes(
        &self,
        repo_id: i64,
        hashes: &Vec<String>,
    ) -> Result<Vec<git_commit::Model>, MegaError> {
        Ok(git_commit::Entity::find()
            .filter(git_commit::Column::RepoId.eq(repo_id))
            .filter(git_commit::Column::CommitId.is_in(hashes))
            .all(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_commits_by_repo_id(
        &self,
        repo_id: i64,
    ) -> Result<impl Stream<Item = Result<git_commit::Model, DbErr>> + Send + '_, MegaError> {
        let stream = git_commit::Entity::find()
            .filter(git_commit::Column::RepoId.eq(repo_id))
            .stream(self.get_connection())
            .await
            .unwrap();
        Ok(stream)
    }

    pub async fn get_last_commit_by_repo_id(
        &self,
        repo_id: i64,
    ) -> Result<Option<git_commit::Model>, MegaError> {
        let one = git_commit::Entity::find()
            .filter(git_commit::Column::RepoId.eq(repo_id))
            .order_by_desc(git_commit::Column::CreatedAt)
            .one(self.get_connection())
            .await?;
        Ok(one)
    }

    pub async fn get_trees_by_repo_id(
        &self,
        repo_id: i64,
    ) -> Result<impl Stream<Item = Result<git_tree::Model, DbErr>> + '_ + Send, MegaError> {
        Ok(git_tree::Entity::find()
            .filter(git_tree::Column::RepoId.eq(repo_id))
            .stream(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_trees_by_hashes(
        &self,
        repo_id: i64,
        hashes: Vec<String>,
    ) -> Result<Vec<git_tree::Model>, MegaError> {
        Ok(git_tree::Entity::find()
            .filter(git_tree::Column::RepoId.eq(repo_id))
            .filter(git_tree::Column::TreeId.is_in(hashes))
            .all(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_tree_by_hash(
        &self,
        repo_id: i64,
        hash: &str,
    ) -> Result<Option<git_tree::Model>, MegaError> {
        Ok(git_tree::Entity::find()
            .filter(git_tree::Column::RepoId.eq(repo_id))
            .filter(git_tree::Column::TreeId.eq(hash))
            .one(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_blobs_by_repo_id(
        &self,
        repo_id: i64,
    ) -> Result<impl Stream<Item = Result<git_blob::Model, DbErr>> + '_ + Send, MegaError> {
        Ok(git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .stream(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_blobs_by_hashes(
        &self,
        repo_id: i64,
        hashes: Vec<String>,
    ) -> Result<Vec<git_blob::Model>, MegaError> {
        Ok(git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .filter(git_blob::Column::BlobId.is_in(hashes))
            .all(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_tags_by_repo_id(
        &self,
        repo_id: i64,
    ) -> Result<Vec<git_tag::Model>, MegaError> {
        Ok(git_tag::Entity::find()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .all(self.get_connection())
            .await
            .unwrap())
    }

    /// Paginated annotated tags for a given import repo id.
    pub async fn list_tags_by_repo_with_page(
        &self,
        repo_id: i64,
        page: Pagination,
    ) -> Result<(Vec<git_tag::Model>, u64), MegaError> {
        let paginator = git_tag::Entity::find()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .order_by_asc(git_tag::Column::TagName)
            .paginate(self.get_connection(), page.per_page);
        let num_items = paginator.num_items().await?;
        Ok(paginator
            .fetch_page(page.page.saturating_sub(1))
            .await
            .map(|m| (m, num_items))?)
    }

    /// Find a stored annotated tag object by its object id.
    pub async fn get_tag_by_hash(
        &self,
        repo_id: i64,
        tag_id: &str,
    ) -> Result<Option<git_tag::Model>, MegaError> {
        let result = git_tag::Entity::find()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .filter(git_tag::Column::TagId.eq(tag_id.to_string()))
            .one(self.get_connection())
            .await?;
        Ok(result)
    }

    /// Find single tag by repo id and tag name
    pub async fn get_tag_by_repo_and_name(
        &self,
        repo_id: i64,
        name: &str,
    ) -> Result<Option<git_tag::Model>, MegaError> {
        let res = git_tag::Entity::find()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .filter(git_tag::Column::TagName.eq(name.to_string()))
            .one(self.get_connection())
            .await?;
        Ok(res)
    }

    /// Insert a single tag model
    pub async fn insert_tag(&self, tag: git_tag::Model) -> Result<git_tag::Model, MegaError> {
        let am: git_tag::ActiveModel = tag.clone().into();
        git_tag::Entity::insert(am)
            .exec(self.get_connection())
            .await?;
        // load saved model back by tag_id
        let model = git_tag::Entity::find()
            .filter(git_tag::Column::TagId.eq(tag.tag_id.clone()))
            .one(self.get_connection())
            .await?;
        match model {
            Some(m) => Ok(m),
            None => Err(MegaError::Other("Failed to load inserted tag".to_string())),
        }
    }

    /// Delete a tag by repo id and name
    pub async fn delete_tag(&self, repo_id: i64, name: &str) -> Result<(), MegaError> {
        git_tag::Entity::delete_many()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .filter(git_tag::Column::TagName.eq(name.to_string()))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn get_obj_count_by_repo_id(&self, repo_id: i64) -> usize {
        let c_count = git_commit::Entity::find()
            .filter(git_commit::Column::RepoId.eq(repo_id))
            .count(self.get_connection())
            .await
            .unwrap();

        let t_count = git_tree::Entity::find()
            .filter(git_tree::Column::RepoId.eq(repo_id))
            .count(self.get_connection())
            .await
            .unwrap();

        let b_count = git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .count(self.get_connection())
            .await
            .unwrap();

        let tag_count = git_tag::Entity::find()
            .filter(git_tag::Column::RepoId.eq(repo_id))
            .count(self.get_connection())
            .await
            .unwrap();

        (c_count + t_count + b_count + tag_count)
            .try_into()
            .unwrap()
    }
}

fn last_wins_filepaths(pairs: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut map = HashMap::with_capacity(pairs.len());
    for (blob_id, file_path) in pairs {
        map.insert(blob_id, file_path);
    }
    map.into_iter().collect()
}

/// Inclusive lower / exclusive upper bound for `repo_path` values that are
/// strict descendants of `path` (`path/…`).
///
/// `'/'` is ASCII 47 and `'0'` is 48, so `[path/, path0)` is the btree range
/// of keys that start with `path/`.
fn git_repo_descendant_bounds(path: &str) -> (String, String) {
    (format!("{path}/"), format!("{path}0"))
}

#[cfg(test)]
mod tests {
    use callisto::{git_blob, git_repo};
    use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
    use tempfile::TempDir;

    use super::*;
    use crate::tests::test_storage;

    fn blob_row(id: i64, repo_id: i64, blob_id: &str, file_path: &str) -> git_blob::Model {
        git_blob::Model {
            id,
            repo_id,
            blob_id: blob_id.to_string(),
            name: None,
            size: 0,
            created_at: chrono::Utc::now().naive_utc(),
            pack_id: String::new(),
            file_path: file_path.to_string(),
            pack_offset: 0,
            is_delta_in_pack: false,
        }
    }

    async fn insert_blob(stg: &GitDbStorage, model: git_blob::Model) {
        git_blob::Entity::insert(model.into_active_model())
            .exec(stg.get_connection())
            .await
            .expect("insert git_blob");
    }

    async fn filepath_of(stg: &GitDbStorage, repo_id: i64, blob_id: &str) -> Option<String> {
        git_blob::Entity::find()
            .filter(git_blob::Column::RepoId.eq(repo_id))
            .filter(git_blob::Column::BlobId.eq(blob_id))
            .one(stg.get_connection())
            .await
            .unwrap()
            .map(|m| m.file_path)
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_empty_is_noop() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        stg.update_git_blob_filepaths(1, vec![])
            .await
            .expect("empty batch");
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_sets_paths_in_one_repo() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        let repo_id = 7;
        insert_blob(&stg, blob_row(1, repo_id, "blob-a", "")).await;
        insert_blob(&stg, blob_row(2, repo_id, "blob-b", "")).await;

        stg.update_git_blob_filepaths(
            repo_id,
            vec![
                ("blob-a".into(), "src/lib.rs".into()),
                ("blob-b".into(), "Cargo.toml".into()),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            filepath_of(&stg, repo_id, "blob-a").await.as_deref(),
            Some("src/lib.rs")
        );
        assert_eq!(
            filepath_of(&stg, repo_id, "blob-b").await.as_deref(),
            Some("Cargo.toml")
        );
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_is_scoped_to_repo() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_blob(&stg, blob_row(1, 1, "shared-blob", "old-a")).await;
        insert_blob(&stg, blob_row(2, 2, "shared-blob", "old-b")).await;

        stg.update_git_blob_filepaths(1, vec![("shared-blob".into(), "src/lib.rs".into())])
            .await
            .unwrap();

        assert_eq!(
            filepath_of(&stg, 1, "shared-blob").await.as_deref(),
            Some("src/lib.rs")
        );
        assert_eq!(
            filepath_of(&stg, 2, "shared-blob").await.as_deref(),
            Some("old-b")
        );
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_duplicate_blob_keeps_last_path() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_blob(&stg, blob_row(1, 1, "dup", "")).await;

        stg.update_git_blob_filepaths(
            1,
            vec![
                ("dup".into(), "first.rs".into()),
                ("dup".into(), "last.rs".into()),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            filepath_of(&stg, 1, "dup").await.as_deref(),
            Some("last.rs")
        );
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_ignores_missing_blob_id() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        stg.update_git_blob_filepaths(1, vec![("missing".into(), "nope.rs".into())])
            .await
            .expect("missing blob is not an error");
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_chunks_above_batch_size() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        let repo_id = 3;
        let n = <BaseStorage as StorageConnector>::BATCH_CHUNK_SIZE + 1;
        let mut pairs = Vec::with_capacity(n);
        for i in 0..n {
            let blob_id = format!("b{i:04}");
            insert_blob(&stg, blob_row(i as i64 + 1, repo_id, &blob_id, "")).await;
            pairs.push((blob_id, format!("f{i}.rs")));
        }

        stg.update_git_blob_filepaths(repo_id, pairs.clone())
            .await
            .unwrap();

        let last_id = format!("b{:04}", n - 1);
        assert_eq!(
            filepath_of(&stg, repo_id, &last_id).await.as_deref(),
            Some(format!("f{}.rs", n - 1).as_str())
        );
        assert_eq!(
            filepath_of(&stg, repo_id, "b0000").await.as_deref(),
            Some("f0.rs")
        );
    }

    #[tokio::test]
    async fn update_git_blob_filepaths_binds_quotes_in_path() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_blob(&stg, blob_row(1, 1, "q", "")).await;
        stg.update_git_blob_filepaths(1, vec![("q".into(), "foo's/bar.rs".into())])
            .await
            .unwrap();
        assert_eq!(
            filepath_of(&stg, 1, "q").await.as_deref(),
            Some("foo's/bar.rs")
        );
    }

    async fn insert_repo(stg: &GitDbStorage, id: i64, path: &str) {
        stg.save_git_repo(git_repo::Model {
            id,
            repo_path: path.to_string(),
            repo_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        })
        .await
        .expect("insert git_repo");
    }

    fn git_repo_row(id: i64, path: &str, name: &str) -> git_repo::Model {
        git_repo::Model {
            id,
            repo_path: path.to_string(),
            repo_name: name.to_string(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[tokio::test]
    async fn save_git_repo_same_path_returns_existing_id() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        let path = "/third-party/rust/crates/foo/1.0.0";
        let first = stg
            .save_git_repo(git_repo_row(11, path, "1.0.0"))
            .await
            .unwrap();
        let second = stg
            .save_git_repo(git_repo_row(22, path, "other"))
            .await
            .unwrap();
        assert_eq!(first.id, 11);
        assert_eq!(second.id, first.id);
        assert_eq!(second.repo_name, first.repo_name);
    }

    #[tokio::test]
    async fn create_repo_and_save_ref_reuses_repo_id() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        let path = "/third-party/foo";
        stg.create_repo_and_save_ref(path, "foo", "refs/heads/master", "aaa")
            .await
            .unwrap();
        let first = stg
            .find_git_repo_exact_match(path)
            .await
            .unwrap()
            .expect("repo after first create");
        stg.create_repo_and_save_ref(path, "foo", "refs/heads/master", "bbb")
            .await
            .unwrap();
        let second = stg
            .find_git_repo_exact_match(path)
            .await
            .unwrap()
            .expect("repo after second create");
        assert_eq!(first.id, second.id);
    }

    #[test]
    fn git_repo_descendant_bounds_are_prefix_range() {
        assert_eq!(
            git_repo_descendant_bounds("/third-party/rust"),
            (
                "/third-party/rust/".to_string(),
                "/third-party/rust0".to_string()
            )
        );
        let (lo, hi) = git_repo_descendant_bounds("/third-party/rust");
        let crate_path = "/third-party/rust/crates/sw/ay/swayws/1.3.0";
        assert!(crate_path >= lo.as_str() && crate_path < hi.as_str());
        assert!(!("/third-party/rust_v1" >= lo.as_str() && "/third-party/rust_v1" < hi.as_str()));
        assert!(!("/third-party/rust" >= lo.as_str() && "/third-party/rust" < hi.as_str()));
    }

    #[tokio::test]
    async fn nested_conflict_finds_descendant_not_sibling_prefix() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_repo(&stg, 1, "/third-party/rust/crates/sw/ay/swayws/1.3.0").await;
        insert_repo(&stg, 2, "/third-party/rust_v1").await;

        let hit = stg
            .find_nested_import_repo_conflict("/third-party/rust")
            .await
            .unwrap()
            .expect("descendant is a conflict");
        assert_eq!(hit.repo_path, "/third-party/rust/crates/sw/ay/swayws/1.3.0");

        assert!(
            stg.find_nested_import_repo_conflict("/third-party/rust_v1")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            stg.find_nested_import_repo_conflict("/third-party/foo")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            stg.find_nested_import_repo_conflict("/third-party/rust/crates/sw/ay/swayws/1.3.0")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn nested_conflict_finds_ancestor() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_repo(&stg, 1, "/third-party/rust").await;

        let hit = stg
            .find_nested_import_repo_conflict("/third-party/rust/crates/to/ki/tokio/1.0.0")
            .await
            .unwrap()
            .expect("ancestor is a conflict");
        assert_eq!(hit.repo_path, "/third-party/rust");
    }

    #[tokio::test]
    async fn like_path_walks_to_longest_segment_prefix() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_repo(&stg, 1, "/third-party/rust").await;
        insert_repo(&stg, 2, "/third-party/rust/crates/foo/1.0.0").await;

        let exact = stg
            .find_git_repo_like_path("/third-party/rust/crates/foo/1.0.0")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exact.repo_path, "/third-party/rust/crates/foo/1.0.0");

        let under = stg
            .find_git_repo_like_path("/third-party/rust/crates/foo/1.0.0/src/lib.rs")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(under.repo_path, "/third-party/rust/crates/foo/1.0.0");

        let parent = stg
            .find_git_repo_like_path("/third-party/rust/crates/bar/2.0.0")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(parent.repo_path, "/third-party/rust");
    }

    #[tokio::test]
    async fn like_path_does_not_match_string_prefix_sibling() {
        let dir = TempDir::new().unwrap();
        let storage = test_storage(dir.path()).await;
        let stg = storage.git_db_storage();
        insert_repo(&stg, 1, "/third-party/rust").await;

        assert!(
            stg.find_git_repo_like_path("/third-party/rust_v1")
                .await
                .unwrap()
                .is_none()
        );
    }
}
