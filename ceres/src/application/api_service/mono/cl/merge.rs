//! CL merge operations for [`ClApplicationService`](super::service::ClApplicationService).

use std::path::PathBuf;

use callisto::{mega_cl, mega_tree, sea_orm_active_enums::ConvTypeEnum};
use common::{errors::MegaError, utils::MEGA_BRANCH_NAME};
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::EntryMeta,
        object::{commit::Commit, tree::Tree},
    },
};
use jupiter::{
    storage::{base_storage::StorageConnector, mono_storage::RefUpdateData},
    utils::converter::{FromMegaModel, IntoMegaModel},
};
use tracing::debug;

use crate::{
    application::api_service::{
        ApiHandler,
        mono::{ClApplicationService, logic::MonoServiceLogic, types::TreeUpdateResult},
    },
    merge_checker::CheckerRegistry,
};

impl ClApplicationService {
    // This function is intended to be called before merging a CL to ensure it meets all required checks.
    pub(crate) async fn ensure_cl_mergeable(&self, cl: &mega_cl::Model) -> Result<(), MegaError> {
        let check_reg = CheckerRegistry::new(self.storage().clone().into(), cl.username.clone());
        check_reg.run_checks(cl.clone().into()).await?;

        let required_check_types = self
            .storage()
            .cl_service
            .cl_store()
            .get_checks_config_by_path(&cl.path)
            .await?
            .into_iter()
            .filter(|cfg| cfg.required)
            .map(|cfg| cfg.check_type_code)
            .collect::<Vec<_>>();

        let failed_checks = self
            .storage()
            .cl_service
            .cl_store()
            .get_check_result(&cl.link)
            .await?
            .into_iter()
            .filter(|result| {
                result.status == "FAILED"
                    && required_check_types
                        .iter()
                        .any(|required_type| required_type == &result.check_type_code)
            })
            .map(|result| format!("{:?}", result.check_type_code))
            .collect::<Vec<_>>();

        if failed_checks.is_empty() {
            Ok(())
        } else {
            Err(MegaError::Other(format!(
                "CL is unmergeable, failed checks: {}",
                failed_checks.join(", ")
            )))
        }
    }

    pub(crate) async fn ensure_merge_no_file_conflicts(
        &self,
        cl: &mega_cl::Model,
    ) -> Result<(), GitError> {
        let main_ref = self
            .storage()
            .mono_storage()
            .get_main_ref(&cl.path)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
            .ok_or_else(|| GitError::CustomError("Main ref not found".to_string()))?;

        let conflicts = self
            .detect_update_conflicts(cl, &main_ref.ref_commit_hash)
            .await?;
        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(GitError::CustomError(format!(
                "Merge conflict on files: {}",
                conflicts.join(", ")
            )))
        }
    }

    pub async fn merge_cl(&self, username: &str, mut cl: mega_cl::Model) -> Result<(), GitError> {
        crate::application::api_service::mono::cl_merge::prepare_cl_path_for_merge(
            self.git_ops(),
            &mut cl,
        )
        .await
        .map_err(|e| GitError::CustomError(e.to_string()))?;

        self.ensure_cl_mergeable(&cl)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let storage = self.storage().mono_storage();
        let refs = storage
            .get_main_ref(&cl.path)
            .await
            .map_err(|e| GitError::CustomError(format!("Failed to get main ref: {}", e)))?
            .ok_or_else(|| GitError::CustomError("Main ref not found".to_string()))?;

        if cl.from_hash != refs.ref_commit_hash {
            return Err(GitError::CustomError("ref hash conflict".to_owned()));
        }

        self.ensure_merge_no_file_conflicts(&cl).await?;

        self.merge_cl_unchecked(username, cl).await
    }

    /// Apply all CL changes onto the target_head in-memory and emit a single commit on the CL ref.
    /// Merges a CL without checking for conflicts.
    /// Caller is responsible for ensuring no conflicts exist before calling this method.
    pub(crate) async fn merge_cl_unchecked(
        &self,
        username: &str,
        cl: mega_cl::Model,
    ) -> Result<(), GitError> {
        let storage = self.storage().mono_storage();

        let strategy =
            crate::application::api_service::mono::cl_merge::resolve_merge_strategy(self, &cl)
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?;
        tracing::info!(
            cl_link = %cl.link,
            cl_path = %cl.path,
            strategy = strategy.as_str(),
            "Applying CL merge"
        );

        let normalized_path = MonoServiceLogic::clean_path_str(&cl.path);
        let (path, update_chain) = if normalized_path == "/" {
            (PathBuf::from("/"), Vec::new())
        } else {
            let path = PathBuf::from(&normalized_path);
            let parent = path.parent().ok_or_else(|| {
                GitError::CustomError(format!("Invalid CL path: {}", normalized_path))
            })?;
            if crate::application::api_service::mono::cl_merge::needs_path_tree_attach(
                self.git_ops(),
                &path,
            )
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
            {
                self.git()
                    .attach_project_path_to_monorepo_root(&normalized_path)
                    .await
                    .map_err(|e| GitError::CustomError(e.to_string()))?;
                crate::application::api_service::mono::cl_merge::sync_path_prefix_main_refs(
                    self.git_ops(),
                    &normalized_path,
                )
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?;
            }
            let update_chain = self.git().search_tree_for_update(parent).await?;
            (path, update_chain)
        };

        let leaf_tree_id =
            crate::application::api_service::mono::cl_merge::resolve_merge_leaf_tree_id(
                self, &cl, strategy,
            )
            .await?;
        let result = MonoServiceLogic::build_result_by_chain(path, update_chain, leaf_tree_id)?;
        self.apply_update_result(&result, "cl merge generated commit", Some(cl.link.as_str()))
            .await?;

        if normalized_path != "/" {
            storage
                .remove_none_cl_refs(&normalized_path)
                .await
                .map_err(|e| GitError::CustomError(format!("Failed to remove refs: {}", e)))?;
            // TODO: self.clean_dangling_commits().await;
        }
        // add conversation
        self.storage()
            .cl_service
            .conversation_store()
            .add_conversation(&cl.link, username, None, ConvTypeEnum::Merged)
            .await
            .map_err(|e| GitError::CustomError(format!("Failed to add conversation: {}", e)))?;
        // update cl status last
        self.storage()
            .cl_service
            .cl_store()
            .merge_cl(cl.clone())
            .await
            .map_err(|e| GitError::CustomError(format!("Failed to update CL status: {}", e)))?;

        // Invalidate admin cache when .mega_cedar.json is modified.
        if let Ok(files) = self.get_sorted_changed_file_list(&cl.link, None).await {
            let admin_file_modified = files.iter().any(|file| {
                let normalized = file.replace('\\', "/");
                normalized.ends_with(crate::application::api_service::mono::ADMIN_FILE)
            });
            if admin_file_modified {
                self.admin().invalidate_admin_cache().await;
            }
        }

        Ok(())
    }

    pub async fn apply_update_result(
        &self,
        result: &TreeUpdateResult,
        commit_msg: &str,
        cl_link: Option<&str>,
    ) -> Result<String, GitError> {
        let storage = self.storage().mono_storage();
        self.ensure_root_tree_keeps_admin_file(result).await?;
        let mut new_commit_id = String::new();
        let mut commits: Vec<Commit> = Vec::new();

        let paths: Vec<&str> = result.ref_updates.iter().map(|r| r.path.as_str()).collect();

        let cl_refs_formatted = cl_link.map(|cl| format!("refs/cl/{}", cl));
        let cl_refs: Option<Vec<&str>> = cl_refs_formatted
            .as_ref()
            .map(|formatted| vec![formatted.as_str(), MEGA_BRANCH_NAME]);

        let refs = storage
            .get_refs_for_paths_and_cls(&paths, cl_refs.as_deref())
            .await?;

        let mut updates: Vec<RefUpdateData> = Vec::new();

        MonoServiceLogic::process_ref_updates(
            result,
            &refs,
            commit_msg,
            &mut commits,
            &mut updates,
            &mut new_commit_id,
        )?;

        if new_commit_id.is_empty() {
            return Err(GitError::CustomError(
                "no commit_id generated: no matching refs found for the update paths".into(),
            ));
        }

        let txn = self
            .storage()
            .begin_db_transaction()
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let save_trees: Vec<mega_tree::ActiveModel> = result
            .updated_trees
            .clone()
            .into_iter()
            .map(|save_t| {
                let mut tree_model: mega_tree::Model = save_t.into_mega_model(EntryMeta::new());
                tree_model.commit_id.clone_from(&new_commit_id);
                tree_model.into()
            })
            .collect();

        storage
            .save_mega_commits(commits, Some(&txn))
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        storage
            .batch_save_model_with_txn(save_trees, Some(&txn))
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        storage
            .batch_upsert_ref_updates_in_txn(updates, &txn)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        txn.commit()
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        Ok(new_commit_id)
    }

    /// Reject updates that would replace monorepo `/` with a tree missing `.mega_cedar.json`
    /// when the current root tip still has it.
    async fn ensure_root_tree_keeps_admin_file(
        &self,
        result: &TreeUpdateResult,
    ) -> Result<(), GitError> {
        let Some(root_update) = result.ref_updates.iter().find(|u| u.path == "/") else {
            return Ok(());
        };

        let storage = self.storage().mono_storage();
        let Some(current) = storage
            .get_main_ref("/")
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
        else {
            return Ok(());
        };

        let current_tree = storage
            .get_tree_by_hash(&current.ref_tree_hash)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
            .ok_or_else(|| GitError::CustomError("Root tree not found".into()))?;
        let current_tree = Tree::from_mega_model(current_tree);
        let current_has_admin = current_tree
            .tree_items
            .iter()
            .any(|item| item.name == crate::application::api_service::mono::ADMIN_FILE);
        if !current_has_admin {
            return Ok(());
        }

        let new_tree = if let Some(t) = result
            .updated_trees
            .iter()
            .find(|t| t.id == root_update.tree_id)
        {
            t.clone()
        } else {
            let model = storage
                .get_tree_by_hash(&root_update.tree_id.to_string())
                .await
                .map_err(|e| GitError::CustomError(e.to_string()))?
                .ok_or_else(|| {
                    GitError::CustomError(format!(
                        "Proposed root tree not found: {}",
                        root_update.tree_id
                    ))
                })?;
            Tree::from_mega_model(model)
        };

        let new_has_admin = new_tree
            .tree_items
            .iter()
            .any(|item| item.name == crate::application::api_service::mono::ADMIN_FILE);
        if !new_has_admin {
            return Err(GitError::CustomError(format!(
                "Refusing to update monorepo root: new tree is missing {}",
                crate::application::api_service::mono::ADMIN_FILE
            )));
        }
        Ok(())
    }

    /// Ensure `refs/cl/{cl_link}` exists, creating it from the CL tip when missing.
    ///
    /// Push / web-edit paths can leave the mega_cl row and CL ref out of sync (or skip the
    /// ref entirely on empty packs). Merge and update-branch need the ref to exist.
    pub(crate) async fn ensure_cl_ref_exists(
        &self,
        cl_link: &str,
    ) -> Result<callisto::mega_refs::Model, GitError> {
        let storage = self.storage().mono_storage();
        let cl = self
            .storage()
            .cl_storage()
            .get_cl(cl_link)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
            .ok_or_else(|| GitError::CustomError(format!("CL not found: {cl_link}")))?;

        let commit = storage
            .get_commit_by_hash(&cl.to_hash)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?
            .ok_or_else(|| GitError::CustomError(format!("Commit not found: {}", cl.to_hash)))?;

        storage
            .ensure_cl_ref(&cl.path, cl_link, &cl.to_hash, &commit.tree)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))
    }

    /// Apply update result but only update the CL ref (never main).
    /// Optionally override the parent commit for the first created commit (used by rebase).
    pub(crate) async fn apply_update_result_cl_only(
        &self,
        result: &TreeUpdateResult,
        commit_msg: &str,
        cl_link: &str,
        parent_override: Option<ObjectHash>,
    ) -> Result<String, GitError> {
        let storage = self.storage().mono_storage();
        self.ensure_root_tree_keeps_admin_file(result).await?;
        let mut new_commit_id = String::new();
        let mut commits: Vec<Commit> = Vec::new();

        let cl_ref = self.ensure_cl_ref_exists(cl_link).await?;

        let mut updates: Vec<RefUpdateData> = Vec::new();

        MonoServiceLogic::process_ref_updates_cl_only(
            result,
            &cl_ref,
            commit_msg,
            parent_override,
            &mut commits,
            &mut updates,
            &mut new_commit_id,
        )?;

        if new_commit_id.is_empty() {
            debug!(
                cl_link,
                ref_name = %cl_ref.ref_name,
                ref_path = %cl_ref.path,
                commit_msg,
                "apply_update_result_cl_only: no commit_id generated"
            );
            return Err(GitError::CustomError(
                "no commit_id generated: no matching refs found for the update paths".into(),
            ));
        }

        storage
            .batch_update_by_path_concurrent(updates)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        storage
            .save_mega_commits(commits, None)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let save_trees: Vec<mega_tree::ActiveModel> = result
            .updated_trees
            .clone()
            .into_iter()
            .map(|save_t| {
                let mut tree_model: mega_tree::Model = save_t.into_mega_model(EntryMeta::new());
                tree_model.commit_id.clone_from(&new_commit_id);
                tree_model.into()
            })
            .collect();

        storage
            .batch_save_model(save_trees)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        Ok(new_commit_id)
    }
}
