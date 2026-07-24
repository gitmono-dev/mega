//! Monorepo entry creation and file edit operations for [`MonoApiService`](super::service::MonoApiService).

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use callisto::mega_tree;
use common::utils::MEGA_BRANCH_NAME;
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::EntryMeta,
        object::{
            blob::Blob,
            commit::Commit,
            tree::{Tree, TreeItem, TreeItemMode},
        },
    },
};
use jupiter::{
    storage::base_storage::StorageConnector,
    utils::converter::{IntoMegaModel, generate_git_keep_with_timestamp},
};

use crate::{
    application::{
        api_service::{
            ApiHandler,
            mono::{
                MonoApiService,
                logic::{MonoServiceLogic, path_not_exist_re},
                types::{CreateEntryUpdate, TreeUpdateResult},
            },
            tree_ops,
        },
        code_edit::{on_edit::OneditCodeEdit, utils as edit_utils},
    },
    model::git::{CreateEntryInfo, CreateEntryResult, EditFilePayload, EditFileResult},
};

impl MonoApiService {
    pub(crate) async fn prepare_create_entry_update(
        &self,
        entry_info: &CreateEntryInfo,
    ) -> Result<CreateEntryUpdate, GitError> {
        let path = PathBuf::from(&entry_info.path);
        let mut save_trees = vec![];
        let file_content = if entry_info.is_directory {
            None
        } else {
            Some(entry_info.content.as_deref().ok_or_else(|| {
                GitError::CustomError("content is required for file creation".to_string())
            })?)
        };

        // Try to get the update chain for the given path.
        // If the path exists, return an empty missing_parts and prefix.
        // If part of the path does not exist, extract the missing segments (missing_parts),
        // determine the valid existing prefix, and rebuild the update_chain from that prefix.
        let (missing_parts, prefix, mut update_chain) =
            match self.search_tree_for_update(&path).await {
                Ok(chain) => (Vec::new(), "", chain),
                Err(err) => {
                    // If search_tree_for_update failed, try to extract the
                    // portion of the path that does not exist from the
                    // error message. The error message is expected to
                    // contain a substring like: Path '.../missing' not exist
                    // We capture that substring to determine which segments
                    // need to be created.
                    let err_str = err.to_string();
                    let extracted = path_not_exist_re()
                        .captures(&err_str)
                        .map(|caps| caps[1].to_string())
                        .ok_or_else(|| {
                            GitError::CustomError(format!("Path resolution failed: {err_str}"))
                        })?;

                    // missing_parts: the trailing path segments after the
                    // first occurrence of the extracted non-existent path.
                    // Example: entry_info.path = "a/b/c/d" and extracted = "c/d"
                    // Then missing_parts = ["c", "d"]
                    let missing_parts = entry_info
                        .path
                        .find(&extracted)
                        .map(|pos| &entry_info.path[pos..])
                        .map(|sub| sub.split('/').collect::<Vec<_>>())
                        .unwrap_or_default();

                    if missing_parts.is_empty() {
                        return Err(GitError::CustomError(format!(
                            "Missing path segments for '{}': {err_str}",
                            entry_info.path
                        )));
                    }

                    // prefix: the valid existing path before the missing parts.
                    // Using the same example above, prefix = "a/b/"
                    let prefix = entry_info
                        .path
                        .find(&extracted)
                        .map(|pos| &entry_info.path[..pos])
                        .unwrap_or("");

                    // Rebuild the update chain starting from the valid prefix
                    // so subsequent operations only update from that known
                    // existing tree downward.
                    let chain = self.search_tree_for_update(Path::new(prefix)).await?;
                    (missing_parts, prefix, chain)
                }
            };

        let target_items = update_chain
            .pop()
            .ok_or_else(|| GitError::CustomError("Empty update chain".to_string()))?
            .tree_items
            .clone();

        // If there are no missing parts, we are inserting directly into an
        // existing tree. This branch handles both creating a new file or
        // creating a new directory in the target tree.
        let (update_result, blob, entry_oid, repo_path) = if missing_parts.is_empty() {
            let mut target_items = target_items;

            // Check for duplicate
            let is_tree_mode = if entry_info.is_directory {
                TreeItemMode::Tree
            } else {
                TreeItemMode::Blob
            };
            if target_items
                .iter()
                .any(|x| x.mode == is_tree_mode && x.name == entry_info.name)
            {
                return Err(GitError::CustomError("Duplicate name".to_string()));
            }

            // Create a new tree item based on whether it's a directory or file
            let (new_item, blob, entry_oid) = if entry_info.is_directory {
                // For a new directory, create a .gitkeep blob so the
                // directory can be represented as a tree with at least
                // one blob entry. The blob contains a timestamp so it's
                // unique.
                let blob = generate_git_keep_with_timestamp();
                let tree_item = TreeItem {
                    mode: TreeItemMode::Blob,
                    id: blob.id,
                    name: String::from(".gitkeep"),
                };
                let new_dir_tree = Tree::from_tree_items(vec![tree_item]).unwrap();
                save_trees.push(new_dir_tree.clone());
                let entry_oid = new_dir_tree.id;
                (
                    TreeItem {
                        mode: TreeItemMode::Tree,
                        id: new_dir_tree.id,
                        name: entry_info.name.clone(),
                    },
                    blob,
                    entry_oid,
                )
            } else {
                let content = file_content
                    .ok_or_else(|| GitError::CustomError("Missing file content".to_string()))?;
                let blob = Blob::from_content(content);
                let entry_oid = blob.id;
                (
                    TreeItem {
                        mode: TreeItemMode::Blob,
                        id: blob.id,
                        name: entry_info.name.clone(),
                    },
                    blob,
                    entry_oid,
                )
            };

            target_items.push(new_item);
            target_items.sort_by(|a, b| a.name.cmp(&b.name));
            let target_tree = Tree::from_tree_items(target_items).unwrap();
            save_trees.push(target_tree.clone());

            // Build update instructions for parent trees and refs.
            // build_result_by_chain walks the update_chain (parent trees)
            // and prepares the list of updated trees and ref updates
            // that must be applied to persist the change.
            let update_result = MonoServiceLogic::build_result_by_chain(
                if prefix.is_empty() {
                    path.clone()
                } else {
                    PathBuf::from(prefix)
                },
                update_chain,
                target_tree.id,
            )?;
            let repo_path = if prefix.is_empty() {
                path.clone()
            } else {
                PathBuf::from(prefix)
            };
            (update_result, blob, entry_oid, repo_path)
        } else {
            // If missing_parts is not empty, we must create intermediate
            // directories (trees) for each missing segment. This branch
            // constructs the leaf tree first and then wraps it with
            // additional trees for each missing path component up to the
            // existing prefix.
            // Create a new tree item based on whether it's a directory or file
            let (leaf_item, blob, entry_oid) = if entry_info.is_directory {
                // Create .gitkeep blob and an initial tree for the new
                // directory leaf. This represents the directory's own
                // tree object which will be nested under new parent trees.
                let blob = generate_git_keep_with_timestamp();
                let tree_item = TreeItem {
                    mode: TreeItemMode::Blob,
                    id: blob.id,
                    name: String::from(".gitkeep"),
                };
                let new_dir_tree = Tree::from_tree_items(vec![tree_item]).unwrap();
                save_trees.push(new_dir_tree.clone());
                let entry_oid = new_dir_tree.id;
                (
                    TreeItem {
                        mode: TreeItemMode::Tree,
                        id: new_dir_tree.id,
                        name: entry_info.name.clone(),
                    },
                    blob,
                    entry_oid,
                )
            } else {
                let content = file_content
                    .ok_or_else(|| GitError::CustomError("Missing file content".to_string()))?;
                let blob = Blob::from_content(content);
                let entry_oid = blob.id;
                (
                    TreeItem {
                        mode: TreeItemMode::Blob,
                        id: blob.id,
                        name: entry_info.name.clone(),
                    },
                    blob,
                    entry_oid,
                )
            };

            let mut current_tree = Tree::from_tree_items(vec![leaf_item]).unwrap();
            save_trees.push(current_tree.clone());

            // Wrap the leaf tree with trees for each missing parent segment.
            // We iterate the missing parts in reverse (from leaf's parent up
            // to the topmost missing segment) and create a tree object for
            // each level that points to the previously built child tree.
            let missing_len = missing_parts.len();
            for part in missing_parts.iter().rev().take(missing_len - 1) {
                let sub_item = TreeItem {
                    mode: TreeItemMode::Tree,
                    id: current_tree.id,
                    name: part.to_string(),
                };

                current_tree = Tree::from_tree_items(vec![sub_item]).unwrap();
                save_trees.push(current_tree.clone());
            }

            // top_part is the highest-level missing segment (closest to the
            // existing prefix). We'll insert this as a child into the
            // existing target_items collected from the update chain.
            let top_part = missing_parts
                .first()
                .expect("missing_parts is non-empty by branch condition")
                .to_string();
            let top_item = TreeItem {
                mode: TreeItemMode::Tree,
                id: current_tree.id,
                name: top_part.clone(),
            };

            let mut target_items = target_items;

            // Check for duplicate
            if target_items
                .iter()
                .any(|x| x.mode == TreeItemMode::Tree && x.name == top_part)
            {
                return Err(GitError::CustomError("Duplicate name".to_string()));
            }

            target_items.push(top_item);
            target_items.sort_by(|a, b| a.name.cmp(&b.name));
            let target_tree = Tree::from_tree_items(target_items).unwrap();
            save_trees.push(target_tree.clone());

            // After constructing the nested trees, build update instructions
            // and apply them to update the parent trees and refs so the
            // new nested directory/file is persisted in the repository.
            let update_result = MonoServiceLogic::build_result_by_chain(
                PathBuf::from(prefix),
                update_chain,
                target_tree.id,
            )?;
            let repo_path = PathBuf::from(prefix);
            (update_result, blob, entry_oid, repo_path)
        };

        Ok(CreateEntryUpdate {
            update_result,
            blob,
            entry_oid,
            repo_path,
            save_trees,
        })
    }

    pub(crate) fn build_entry_path(path: &str, name: &str) -> String {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "/" {
            format!("/{name}")
        } else {
            format!("{trimmed}/{name}")
        }
    }

    pub(crate) fn ref_update_tree_id_for_path(
        result: &TreeUpdateResult,
        repo_path: &str,
    ) -> Option<ObjectHash> {
        let normalized = MonoServiceLogic::normalize_repo_path(repo_path).ok()?;
        result
            .ref_updates
            .iter()
            .find(|update| update.path == normalized)
            .map(|update| update.tree_id)
    }
    pub(crate) async fn save_file_edit_impl(
        &self,
        payload: EditFilePayload,
    ) -> Result<EditFileResult, GitError> {
        let file_path = PathBuf::from("/").join(PathBuf::from(&payload.path));
        let parent_path = file_path
            .parent()
            .ok_or_else(|| GitError::CustomError("Invalid file path".to_string()))?;
        // Bind the CL to the deepest path that already has main (e.g. /toolchains),
        // not the Buck build root `/`. Root-bound CLs can wipe /.mega_cedar.json on merge.
        let parent_repo_path = MonoServiceLogic::subtree_ref_path(parent_path)
            .map_err(|e| GitError::CustomError(e.to_string()))?;
        let cl_path = edit_utils::resolve_cl_path_for_edit(self.storage(), &parent_repo_path)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let parent_tree = tree_ops::search_tree_by_path(self, parent_path, None)
            .await?
            .ok_or(GitError::CustomError(format!(
                "invalid repo_path {}, Parent tree not found",
                cl_path
            )))?;

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| GitError::CustomError("Invalid file name".to_string()))?;

        let dest_name =
            if let Some(new_path) = payload.new_path.as_deref().filter(|p| !p.is_empty()) {
                let dest_path = PathBuf::from("/").join(PathBuf::from(new_path));
                let dest_parent = dest_path
                    .parent()
                    .ok_or_else(|| GitError::CustomError("Invalid new file path".to_string()))?;
                if dest_parent != parent_path {
                    return Err(GitError::CustomError(
                    "Rename across directories is not supported; keep the file in the same folder"
                        .to_string(),
                ));
                }
                dest_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| GitError::CustomError("Invalid new file name".to_string()))?
                    .to_string()
            } else {
                file_name.to_string()
            };

        let _current_item = parent_tree
            .tree_items
            .iter()
            .find(|x| x.name == file_name && x.mode == TreeItemMode::Blob)
            .ok_or_else(|| GitError::CustomError("[code:404] File not found".to_string()))?;

        // Create new blob and build update result up to root
        let new_blob = Blob::from_content(&payload.content);
        let new_tree = MonoServiceLogic::update_or_rename_tree_blob(
            parent_tree.into(),
            file_name,
            &dest_name,
            new_blob.id,
        )?;

        let mut update_chain = self.search_tree_for_update(parent_path).await?;
        let _target_tree = update_chain
            .pop()
            .ok_or_else(|| GitError::CustomError("Empty update chain".to_string()))?;
        let update_result = MonoServiceLogic::build_result_by_chain(
            parent_path.to_path_buf(),
            update_chain,
            new_tree.id,
        )?;
        let target_tree_id = Self::ref_update_tree_id_for_path(&update_result, &cl_path)
            .ok_or_else(|| {
                GitError::CustomError(format!("Missing updated tree for CL path {cl_path}"))
            })?;

        let src_commit = edit_utils::get_repo_main_latest_commit(self.storage(), &cl_path).await?;
        let dst_commit = Commit::from_tree_id(
            target_tree_id,
            vec![
                ObjectHash::from_str(&src_commit.id.to_string()).map_err(|e| {
                    GitError::CustomError(format!("Invalid commit hash {}: {e}", src_commit.id))
                })?,
            ],
            &payload.commit_message,
        );
        let new_commit_id = dst_commit.id.to_string();

        let username = payload
            .author_username
            .clone()
            .unwrap_or("Anonymous".to_string());

        self.storage()
            .mono_service
            .mono_storage
            .save_mega_commits(vec![dst_commit], None)
            .await?;

        let mut all_trees = vec![new_tree];
        all_trees.extend(update_result.updated_trees);
        let save_trees: Vec<mega_tree::ActiveModel> = all_trees
            .into_iter()
            .map(|save_t| {
                let mut tree_model: mega_tree::Model = save_t.into_mega_model(EntryMeta::new());
                tree_model.commit_id.clone_from(&new_commit_id);
                tree_model.into()
            })
            .collect();

        self.storage()
            .mono_service
            .mono_storage
            .batch_save_model(save_trees)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let editor = OneditCodeEdit::from(
            &cl_path,
            MEGA_BRANCH_NAME
                .strip_prefix("refs/heads/")
                .unwrap_or(MEGA_BRANCH_NAME),
            &src_commit.id.to_string(),
            self,
            self.storage().mono_storage(),
        );
        let cl = editor
            .find_or_create_cl_for_edit(
                self.storage(),
                &editor,
                payload.mode,
                &new_commit_id,
                &username,
            )
            .await?;

        self.storage()
            .mono_service
            .save_blobs(&new_commit_id, vec![new_blob.clone()])
            .await?;

        if !payload.skip_build {
            self.trigger_build_for_cl(&editor, &cl, &username).await?;
        }

        Ok(EditFileResult {
            commit_id: new_commit_id,
            new_oid: new_blob.id.to_string(),
            path: cl_path,
            cl_link: Some(cl.link),
        })
    }

    /// Creates a new file or directory in the monorepo based on the provided file information.
    ///
    /// # Arguments
    ///
    /// * `entry_info` - Information about the file or directory to create.
    ///
    /// # Returns
    ///
    /// Returns commit metadata on success, or a `GitError` on failure.
    pub(crate) async fn create_monorepo_entry_impl(
        &self,
        entry_info: CreateEntryInfo,
    ) -> Result<CreateEntryResult, GitError> {
        let storage = self.storage().mono_storage();
        let CreateEntryUpdate {
            update_result,
            blob,
            entry_oid,
            repo_path,
            mut save_trees,
        } = self.prepare_create_entry_update(&entry_info).await?;

        let repo_path_str = MonoServiceLogic::subtree_ref_path(&repo_path)
            .map_err(|e| GitError::CustomError(e.to_string()))?;
        // Bind CL to deepest path with main (e.g. /toolchains), not Buck root `/`.
        let cl_path = edit_utils::resolve_cl_path_for_edit(self.storage(), &repo_path_str)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        let src_commit = edit_utils::get_repo_main_latest_commit(self.storage(), &cl_path).await?;
        let base_commit = ObjectHash::from_str(&src_commit.id.to_string()).map_err(|e| {
            GitError::CustomError(format!("Invalid commit hash {}: {e}", src_commit.id))
        })?;
        let target_tree_id = Self::ref_update_tree_id_for_path(&update_result, &cl_path)
            .ok_or_else(|| {
                GitError::CustomError(format!("Missing updated tree for CL path {cl_path}"))
            })?;
        let dst_commit =
            Commit::from_tree_id(target_tree_id, vec![base_commit], &entry_info.commit_msg());
        let new_commit_id = dst_commit.id.to_string();

        let username = entry_info
            .author_username
            .clone()
            .unwrap_or("Anonymous".to_string());

        let new_oid = entry_oid.to_string();

        let mut all_trees = update_result.updated_trees;
        all_trees.append(&mut save_trees);
        let save_trees: Vec<mega_tree::ActiveModel> = all_trees
            .into_iter()
            .map(|save_t| {
                let mut tree_model: mega_tree::Model = save_t.into_mega_model(EntryMeta::new());
                tree_model.commit_id.clone_from(&new_commit_id);
                tree_model.into()
            })
            .collect();
        self.storage()
            .mono_service
            .save_blobs(&new_commit_id, vec![blob])
            .await?;

        storage
            .batch_save_model(save_trees)
            .await
            .map_err(|e| GitError::CustomError(e.to_string()))?;

        self.storage()
            .mono_service
            .mono_storage
            .save_mega_commits(vec![dst_commit], None)
            .await?;

        let editor = OneditCodeEdit::from(
            &cl_path,
            MEGA_BRANCH_NAME
                .strip_prefix("refs/heads/")
                .unwrap_or(MEGA_BRANCH_NAME),
            &src_commit.id.to_string(),
            self,
            self.storage().mono_storage(),
        );
        let cl = editor
            .find_or_create_cl_for_edit(
                self.storage(),
                &editor,
                entry_info.mode.clone(),
                &new_commit_id,
                &username,
            )
            .await?;

        if !entry_info.skip_build {
            self.trigger_build_for_cl(&editor, &cl, &username).await?;
        }

        let entry_path = Self::build_entry_path(&entry_info.path, &entry_info.name);

        Ok(CreateEntryResult {
            commit_id: new_commit_id,
            new_oid,
            path: entry_path,
            cl_link: Some(cl.link),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use git_internal::hash::ObjectHash;

    use crate::application::api_service::mono::{
        MonoApiService,
        types::{RefUpdate, TreeUpdateResult},
    };

    #[test]
    fn test_save_file_edit_uses_build_repo_root_tree_from_ref_updates() {
        let build_root_tree =
            ObjectHash::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let nested_tree = ObjectHash::from_str("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let result = TreeUpdateResult {
            updated_trees: vec![],
            ref_updates: vec![
                RefUpdate {
                    path: "/project/buck2_test/src".to_string(),
                    tree_id: nested_tree,
                },
                RefUpdate {
                    path: "/project/buck2_test".to_string(),
                    tree_id: build_root_tree,
                },
            ],
        };

        let selected = MonoApiService::ref_update_tree_id_for_path(&result, "/project/buck2_test");
        assert_eq!(selected, Some(build_root_tree));
    }

    #[test]
    fn test_create_monorepo_entry_uses_normalized_build_repo_root_tree() {
        let build_root_tree =
            ObjectHash::from_str("cccccccccccccccccccccccccccccccccccccccc").unwrap();
        let result = TreeUpdateResult {
            updated_trees: vec![],
            ref_updates: vec![RefUpdate {
                path: "/project/buck2_test".to_string(),
                tree_id: build_root_tree,
            }],
        };

        let selected = MonoApiService::ref_update_tree_id_for_path(&result, "/project/buck2_test/");
        assert_eq!(selected, Some(build_root_tree));
    }
}
