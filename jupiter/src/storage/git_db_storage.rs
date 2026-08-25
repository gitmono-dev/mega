use std::ops::Deref;

use api_model::common::Pagination;
use callisto::{
    git_blob, git_commit, git_repo, git_tag, git_tree, import_refs,
    sea_orm_active_enums::RefTypeEnum,
};
use common::{
    errors::MegaError,
    utils::{
        generate_id, nested_import_repo_conflict_message,
    },
};
use futures::Stream;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbBackend, DbErr, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, QueryTrait, Set, TransactionTrait,
    sea_query::Expr,
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
            let repo_id = generate_id();
            let repo = git_repo::Model {
                id: repo_id,
                repo_path: repo_path.to_string(),
                repo_name: repo_name.to_string(),
                created_at: chrono::Utc::now().naive_utc(),
                updated_at: chrono::Utc::now().naive_utc(),
            };
            self.save_git_repo(repo).await?;
            repo_id
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
        blob_id: &String,
        file_path: &str,
    ) -> Result<(), MegaError> {
        if let Some(model) = git_blob::Entity::find()
            .filter(git_blob::Column::BlobId.eq(blob_id))
            .one(self.get_connection())
            .await?
        {
            let mut active: git_blob::ActiveModel = model.into();

            active.file_path = Set(file_path.to_string());

            active.update(self.get_connection()).await?;
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
        if let Some(descendant) = git_repo::Entity::find()
            .filter(git_repo::Column::RepoPath.like(format!("{path}/%")))
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

    /// Finds a Git repository with a path that matches the beginning of the provided repository path using a LIKE query.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - A string slice that holds the beginning of the path of the repository to search for.
    ///
    /// # Returns
    ///
    /// A `Result` containing an `Option` with the Git repository model if found, or `None` if not found.
    /// Returns a `MegaError` if an error occurs during the search.
    pub async fn find_git_repo_like_path(
        &self,
        repo_path: &str,
    ) -> Result<Option<git_repo::Model>, MegaError> {
        let query = git_repo::Entity::find()
            .filter(Expr::cust(format!("'{repo_path}' LIKE repo_path || '%'")))
            .order_by_desc(Expr::cust("LENGTH(repo_path)"));
        tracing::debug!("{}", query.build(DbBackend::Postgres).to_string());
        let result = query.one(self.get_connection()).await?;
        Ok(result)
    }

    pub async fn save_git_repo(&self, repo: git_repo::Model) -> Result<(), MegaError> {
        let a_model = repo.into_active_model();
        git_repo::Entity::insert(a_model)
            .exec(self.get_connection())
            .await
            .map_err(|e| MegaError::Other(format!("Failed to insert git_repo: {e}")))?;
        Ok(())
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
