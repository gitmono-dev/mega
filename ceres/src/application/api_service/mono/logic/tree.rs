use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use callisto::mega_refs;
use common::utils::MEGA_BRANCH_NAME;
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::object::{
        commit::Commit,
        tree::{Tree, TreeItem},
    },
};
use jupiter::storage::mono_storage::RefUpdateData;

use super::MonoServiceLogic;
use crate::application::api_service::mono::types::{RefUpdate, TreeUpdateResult};

impl MonoServiceLogic {
    pub fn update_tree_hash(
        tree: Arc<Tree>,
        name: &str,
        target_hash: ObjectHash,
    ) -> Result<Tree, GitError> {
        let index = tree
            .tree_items
            .iter()
            .position(|item| item.name == name)
            .ok_or_else(|| GitError::CustomError(format!("Tree item '{}' not found", name)))?;
        let mut items = tree.tree_items.clone();
        items[index].id = target_hash;
        Tree::from_tree_items(items).map_err(|_| GitError::CustomError("Invalid tree".to_string()))
    }

    /// Update a blob's content hash and optionally rename it within the same tree.
    pub fn update_or_rename_tree_blob(
        tree: Arc<Tree>,
        old_name: &str,
        new_name: &str,
        target_hash: ObjectHash,
    ) -> Result<Tree, GitError> {
        if old_name == new_name {
            return Self::update_tree_hash(tree, old_name, target_hash);
        }

        let index = tree
            .tree_items
            .iter()
            .position(|item| item.name == old_name)
            .ok_or_else(|| GitError::CustomError(format!("Tree item '{old_name}' not found")))?;

        if tree.tree_items.iter().any(|item| item.name == new_name) {
            return Err(GitError::CustomError(format!(
                "Duplicate name '{new_name}'"
            )));
        }

        let mut items = tree.tree_items.clone();
        items[index].name = new_name.to_string();
        items[index].id = target_hash;
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Tree::from_tree_items(items).map_err(|_| GitError::CustomError("Invalid tree".to_string()))
    }

    /// Walk an update chain from leaf to root, returning rebuilt trees and the new root tree id.
    pub fn propagate_tree_chain(
        mut path: PathBuf,
        mut update_chain: Vec<Arc<Tree>>,
        mut updated_tree_hash: ObjectHash,
    ) -> Result<(Vec<Tree>, ObjectHash), GitError> {
        let mut updated_trees = Vec::new();
        while let Some(tree) = update_chain.pop() {
            let cloned_path = path.clone();
            let name = cloned_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| GitError::CustomError("Invalid path".into()))?;
            path.pop();

            let new_tree = Self::update_tree_hash(tree, name, updated_tree_hash)?;
            updated_tree_hash = new_tree.id;
            updated_trees.push(new_tree);
        }
        Ok((updated_trees, updated_tree_hash))
    }

    /// Update parent trees along the given update chain with the new child tree hash.
    /// This function prepares all updated trees and their associated ref updates.
    /// Trees that do not depend on each other (e.g., sibling directories) can be updated in parallel.
    /// No new commits are created; only tree objects and ref updates are produced.
    pub fn build_result_by_chain(
        mut path: PathBuf,
        mut update_chain: Vec<Arc<Tree>>,
        mut updated_tree_hash: ObjectHash,
    ) -> Result<TreeUpdateResult, GitError> {
        let mut updated_trees = Vec::new();
        let mut ref_updates = Vec::new();
        let mut path_str = path.to_string_lossy().to_string();

        loop {
            let clean_path = MonoServiceLogic::clean_path_str(&path_str);
            let ref_path = if clean_path == "/" || clean_path.starts_with('/') {
                clean_path
            } else {
                format!("/{clean_path}")
            };

            ref_updates.push(RefUpdate {
                path: ref_path,
                tree_id: updated_tree_hash,
            });

            if update_chain.is_empty() {
                break;
            }

            let cloned_path = path.clone();
            let name = cloned_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| GitError::CustomError("Invalid path".into()))?;
            path.pop();
            path_str = path.to_string_lossy().to_string();

            let tree = update_chain
                .pop()
                .ok_or_else(|| GitError::CustomError("Empty update chain".into()))?;

            let new_tree = MonoServiceLogic::update_tree_hash(tree, name, updated_tree_hash)?;
            updated_tree_hash = new_tree.id;
            updated_trees.push(new_tree);
        }

        Ok(TreeUpdateResult {
            updated_trees,
            ref_updates,
        })
    }

    /// Processes all ref updates by creating new commits and updating refs accordingly.
    ///
    /// This method abstracts the entire loop logic for processing ref updates,
    /// creating commits for each update and managing the refs that need to be updated.
    pub fn process_ref_updates(
        result: &TreeUpdateResult,
        refs: &[mega_refs::Model],
        commit_msg: &str,
        commits: &mut Vec<Commit>,
        updates: &mut Vec<RefUpdateData>,
        new_commit_id: &mut String,
    ) -> Result<(), GitError> {
        for update in &result.ref_updates {
            let path_refs: Vec<&mega_refs::Model> =
                refs.iter().filter(|r| r.path == update.path).collect();
            let p_ref = path_refs
                .iter()
                .find(|r| r.ref_name.starts_with("refs/cl/"))
                .copied()
                .or_else(|| {
                    path_refs
                        .iter()
                        .find(|r| r.ref_name == MEGA_BRANCH_NAME)
                        .copied()
                });
            let Some(p_ref) = p_ref else {
                continue;
            };
            let commit = Commit::from_tree_id(
                update.tree_id,
                vec![ObjectHash::from_str(&p_ref.ref_commit_hash).unwrap()],
                commit_msg,
            );
            let commit_id = commit.id.to_string();
            *new_commit_id = commit_id.clone();

            commits.push(commit);

            let mut push_update = |ref_name: &str| {
                updates.push(RefUpdateData {
                    path: p_ref.path.clone(),
                    ref_name: ref_name.to_string(),
                    commit_id: commit_id.to_string(),
                    tree_hash: update.tree_id.to_string(),
                });
            };

            push_update(&p_ref.ref_name);
            if p_ref.ref_name.starts_with("refs/cl/") {
                push_update(MEGA_BRANCH_NAME);
            }
        }

        Ok(())
    }

    /// Processes ref updates but only for CL refs; never touches main and supports chaining parents.
    pub fn process_ref_updates_cl_only(
        result: &TreeUpdateResult,
        cl_ref: &mega_refs::Model,
        commit_msg: &str,
        parent_override: Option<ObjectHash>,
        commits: &mut Vec<Commit>,
        updates: &mut Vec<RefUpdateData>,
        new_commit_id: &mut String,
    ) -> Result<(), GitError> {
        let mut prev_parent: Option<ObjectHash> = None;

        for update in &result.ref_updates {
            let parent_ids = if let Some(prev) = prev_parent {
                vec![prev]
            } else if let Some(po) = parent_override {
                vec![po]
            } else {
                vec![ObjectHash::from_str(&cl_ref.ref_commit_hash).map_err(|_| {
                    GitError::CustomError(format!(
                        "Invalid CL ref hash: {}",
                        cl_ref.ref_commit_hash
                    ))
                })?]
            };

            let commit = Commit::from_tree_id(update.tree_id, parent_ids, commit_msg);
            let commit_id = commit.id;
            *new_commit_id = commit_id.to_string();

            commits.push(commit.clone());
            prev_parent = Some(commit_id);

            updates.push(RefUpdateData {
                path: cl_ref.path.clone(),
                ref_name: cl_ref.ref_name.clone(),
                commit_id: commit_id.to_string(),
                tree_hash: update.tree_id.to_string(),
            });
        }

        Ok(())
    }

    /// Maps each TreeItem in a Tree to its corresponding Commit, if available.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree containing the TreeItems to map.
    /// * `item_to_commit_id` - Mapping from TreeItem id (as string) to commit id.
    /// * `commit_map` - Mapping from commit id to Commit object.
    ///
    /// # Returns
    ///
    /// A HashMap where each TreeItem maps to an Option<Commit>. If a commit cannot
    /// be found, the value is None.
    pub fn map_tree_items_to_commits(
        tree: Tree,
        item_to_commit_id: &HashMap<String, String>,
        commit_map: &HashMap<String, Commit>,
    ) -> HashMap<TreeItem, Option<Commit>> {
        let mut result: HashMap<TreeItem, Option<Commit>> = HashMap::new();

        for item in tree.tree_items {
            if let Some(commit_id) = item_to_commit_id.get(&item.id.to_string()) {
                let commit = commit_map.get(commit_id).cloned();
                if commit.is_none() {
                    tracing::warn!(
                        item_name = %item.name,
                        item_mode = ?item.mode,
                        commit_id = %commit_id,
                        "failed fetch from commit map"
                    );
                }
                result.insert(item, commit);
            } else {
                result.insert(item, None);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

    use git_internal::{
        hash::ObjectHash,
        internal::object::{
            commit::Commit,
            signature::{Signature, SignatureType},
            tree::{Tree, TreeItem, TreeItemMode},
        },
    };

    use super::MonoServiceLogic;

    #[test]
    fn test_update_tree_hash() {
        let item = TreeItem::new(
            TreeItemMode::Blob,
            ObjectHash::from_str("1234567890123456789012345678901234567890").unwrap(),
            "path".to_string(),
        );

        let tree = Tree::from_tree_items(vec![item]).expect("tree should build");
        let tree = Arc::new(tree);

        let new_hash = ObjectHash::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();

        let new_tree = MonoServiceLogic::update_tree_hash(tree, "path", new_hash)
            .expect("update_tree_hash should succeed");

        assert_eq!(new_tree.tree_items.len(), 1);
        assert_eq!(new_tree.tree_items[0].id, new_hash);
    }

    #[test]
    fn test_update_or_rename_tree_blob() {
        let old_hash = ObjectHash::from_str("1234567890123456789012345678901234567890").unwrap();
        let new_hash = ObjectHash::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let other_hash = ObjectHash::from_str("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();

        let tree = Arc::new(
            Tree::from_tree_items(vec![
                TreeItem::new(TreeItemMode::Blob, old_hash, "old.txt".to_string()),
                TreeItem::new(TreeItemMode::Blob, other_hash, "sibling.txt".to_string()),
            ])
            .expect("tree should build"),
        );

        let renamed = MonoServiceLogic::update_or_rename_tree_blob(
            Arc::clone(&tree),
            "old.txt",
            "new.txt",
            new_hash,
        )
        .expect("rename should succeed");

        assert_eq!(renamed.tree_items.len(), 2);
        assert!(
            renamed
                .tree_items
                .iter()
                .any(|item| item.name == "new.txt" && item.id == new_hash)
        );
        assert!(!renamed.tree_items.iter().any(|item| item.name == "old.txt"));

        let duplicate =
            MonoServiceLogic::update_or_rename_tree_blob(tree, "old.txt", "sibling.txt", new_hash);
        assert!(duplicate.is_err());
    }

    #[test]
    fn test_build_result_by_chain_logic() {
        let item = TreeItem::new(
            TreeItemMode::Blob,
            ObjectHash::from_str("1234567890123456789012345678901234567890").unwrap(),
            "path".to_string(),
        );

        let tree = Tree::from_tree_items(vec![item]).expect("tree should build");
        let tree_id = tree.id;

        let update_chain = vec![Arc::new(tree)];
        let path = PathBuf::from("/test/path");

        let result = MonoServiceLogic::build_result_by_chain(path, update_chain, tree_id)
            .expect("build_result_by_chain should succeed");

        assert_eq!(result.updated_trees.len(), 1);
        assert_eq!(result.ref_updates.len(), 2);

        let paths: Vec<&str> = result.ref_updates.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"/test/path"));
        assert!(paths.contains(&"/test"));
    }

    #[test]
    fn test_build_result_by_chain_normalizes_relative_paths_for_ref_updates() {
        let old_hash = ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap();
        let updated_child_hash =
            ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap();
        let item = TreeItem::new(TreeItemMode::Tree, old_hash, "src".to_string());
        let tree = Tree::from_tree_items(vec![item]).expect("tree should build");
        let update_chain = vec![Arc::new(tree)];

        let result = MonoServiceLogic::build_result_by_chain(
            PathBuf::from("project/buck2_test/src"),
            update_chain,
            updated_child_hash,
        )
        .expect("build_result_by_chain should succeed");

        let paths: Vec<&str> = result.ref_updates.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"/project/buck2_test/src"));
        assert!(paths.contains(&"/project/buck2_test"));
    }

    #[test]
    fn test_map_tree_items_to_commits() {
        let id1 = ObjectHash::Sha1([1u8; 20]);
        let id2 = ObjectHash::Sha1([2u8; 20]);
        let commit_hash = ObjectHash::Sha1([3u8; 20]);

        let item1 = TreeItem {
            id: id1,
            name: "file1.txt".into(),
            mode: TreeItemMode::Blob,
        };
        let item2 = TreeItem {
            id: id2,
            name: "file2.txt".into(),
            mode: TreeItemMode::Blob,
        };

        let tree = Tree {
            id: ObjectHash::Sha1([9u8; 20]),
            tree_items: vec![item1.clone(), item2.clone()],
        };

        let mut item_to_commit_id = HashMap::new();
        item_to_commit_id.insert(id1.to_string(), commit_hash.to_string());

        let fake_sig = Signature {
            signature_type: SignatureType::Committer,
            name: "tester".into(),
            email: "tester@example.com".into(),
            timestamp: 0,
            timezone: "+0000".into(),
        };

        let commit_a = Commit {
            id: commit_hash,
            tree_id: ObjectHash::Sha1([8u8; 20]),
            parent_commit_ids: vec![],
            author: fake_sig.clone(),
            committer: fake_sig.clone(),
            message: "test commit".into(),
        };

        let mut commit_map = HashMap::new();
        commit_map.insert(commit_hash.to_string(), commit_a.clone());

        let result =
            MonoServiceLogic::map_tree_items_to_commits(tree, &item_to_commit_id, &commit_map);

        assert_eq!(result.get(&item1), Some(&Some(commit_a)));
        assert_eq!(result.get(&item2), Some(&None));
    }
}
