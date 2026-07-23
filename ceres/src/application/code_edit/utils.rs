use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    vec,
};

use callisto::{mega_cl, mega_refs};
use common::{self, errors::MegaError, utils::ZERO_ID};
use git_internal::{
    hash::ObjectHash,
    internal::object::{commit::Commit, tree::Tree},
};
use jupiter::{storage::Storage, utils::converter::FromMegaModel};

use crate::{
    application::api_service::{ApiHandler, commit_ops, mono::MonoServiceLogic},
    model::change_list::ClDiffFile,
};

pub async fn cl_files_list(
    old_files: Vec<(PathBuf, ObjectHash)>,
    new_files: Vec<(PathBuf, ObjectHash)>,
) -> Result<Vec<ClDiffFile>, MegaError> {
    let old_files: HashMap<PathBuf, ObjectHash> = old_files.into_iter().collect();
    let new_files: HashMap<PathBuf, ObjectHash> = new_files.into_iter().collect();
    let unions: HashSet<PathBuf> = old_files.keys().chain(new_files.keys()).cloned().collect();
    let mut res = vec![];
    for path in unions {
        let old_hash = old_files.get(&path);
        let new_hash = new_files.get(&path);
        match (old_hash, new_hash) {
            (None, None) => {}
            (None, Some(new)) => res.push(ClDiffFile::New(path, *new)),
            (Some(old), None) => res.push(ClDiffFile::Deleted(path, *old)),
            (Some(old), Some(new)) => {
                if old == new {
                    continue;
                } else {
                    res.push(ClDiffFile::Modified(path, *old, *new));
                }
            }
        }
    }

    // Sort the results
    res.sort_by(|a, b| {
        a.path()
            .cmp(b.path())
            .then_with(|| a.kind_weight().cmp(&b.kind_weight()))
    });
    Ok(res)
}

pub async fn get_repo_main_latest_commit(
    storage: &Storage,
    repo_path: &str,
) -> Result<Commit, MegaError> {
    let mono_storage = storage.mono_storage();
    let commit_hash = match mono_storage.get_main_ref(repo_path).await {
        Ok(Some(refs)) => refs.ref_commit_hash,
        _ => create_repo_commit(storage, repo_path).await?,
    };
    Ok(Commit::from_mega_model(
        mono_storage
            .get_commit_by_hash(&commit_hash)
            .await?
            .expect("can't fetch commit by hash"),
    ))
}

fn has_buck_root_markers(tree: &Tree) -> bool {
    let names: HashSet<_> = tree
        .tree_items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    // Only explicit Buck markers identify a build root. A `toolchains/` directory
    // alone must not qualify — otherwise edits under `/toolchains` resolve the CL
    // path to `/` and a bad tip can wipe the monorepo root on merge.
    names.contains(".buckroot") && names.contains(".buckconfig")
}

#[cfg(test)]
fn path_has_repo_prefix(path: &str, repo_path: &str) -> bool {
    path == repo_path || path.starts_with(&format!("{repo_path}/"))
}

#[cfg(test)]
pub(crate) fn resolve_repo_root_from_repo_paths(
    path: &str,
    repo_paths: &[String],
) -> Result<Option<String>, MegaError> {
    let normalized_path = MonoServiceLogic::normalize_repo_path(path)?;
    let resolved = repo_paths
        .iter()
        .filter_map(|repo_path| {
            let normalized_repo = MonoServiceLogic::normalize_repo_path(repo_path).ok()?;
            path_has_repo_prefix(&normalized_path, &normalized_repo).then_some(normalized_repo)
        })
        .max_by_key(|repo_path| repo_path.len());
    Ok(resolved)
}

pub async fn resolve_build_repo_root(storage: &Storage, path: &str) -> Result<String, MegaError> {
    let normalized_path = MonoServiceLogic::normalize_repo_path(path)?;
    let candidates = MonoServiceLogic::repo_root_candidates(Path::new(&normalized_path));

    for candidate in &candidates {
        if storage
            .git_db_storage()
            .find_git_repo_exact_match(candidate)
            .await?
            .is_some()
        {
            return Ok(candidate.clone());
        }
    }

    let mono_storage = storage.mono_storage();
    for candidate in candidates {
        let Some(refs) = mono_storage.get_main_ref(&candidate).await? else {
            continue;
        };
        let Some(tree_model) = mono_storage.get_tree_by_hash(&refs.ref_tree_hash).await? else {
            continue;
        };
        let tree = Tree::from_mega_model(tree_model);
        if has_buck_root_markers(&tree) {
            return Ok(candidate);
        }
    }

    Err(MegaError::Other(format!(
        "No repository root found for path {normalized_path}"
    )))
}

/// Resolve the monorepo path that should own a web-edit CL.
///
/// Prefers the deepest ancestor that already has `refs/heads/main`, so edits under
/// `/toolchains` bind to `/toolchains` rather than the Buck monorepo root `/`.
/// Falling back to [`resolve_build_repo_root`] keeps Buck project crates working.
pub async fn resolve_cl_path_for_edit(storage: &Storage, path: &str) -> Result<String, MegaError> {
    let normalized_path = MonoServiceLogic::normalize_repo_path(path)?;
    let mono_storage = storage.mono_storage();
    let candidates = MonoServiceLogic::repo_root_candidates(Path::new(&normalized_path));

    for candidate in &candidates {
        if mono_storage.get_main_ref(candidate).await?.is_some() {
            return Ok(candidate.clone());
        }
    }

    resolve_build_repo_root(storage, &normalized_path).await
}

/// Get list of files changed between from_hash and to_hash commits.
/// Returns paths relative to the CL root directory with forward slashes.
pub async fn get_changed_files<T: ApiHandler>(
    handler: &T,
    cl: &mega_cl::Model,
) -> Result<Vec<String>, MegaError> {
    let to_commit = handler.get_commit_by_hash(&cl.to_hash).await?;
    let old_files = if cl.from_hash == ZERO_ID {
        Vec::new()
    } else {
        let from_commit = handler.get_commit_by_hash(&cl.from_hash).await?;
        commit_ops::collect_commit_blobs(handler, &from_commit).await?
    };
    let new_files = commit_ops::collect_commit_blobs(handler, &to_commit).await?;
    let changed = cl_files_list(old_files, new_files).await?;

    // Normalize CL root path to use forward slashes
    let cl_root = cl.path.to_string().replace('\\', "/");
    let cl_root_normalized = cl_root.trim_start_matches('/');

    let file_paths: Vec<String> = changed
        .iter()
        .map(|f| {
            let full_path = f.path().to_string_lossy().replace('\\', "/");
            let full_path_normalized = full_path.trim_start_matches('/');

            // Strip CL root prefix to get relative path
            if let Some(rel) = full_path_normalized.strip_prefix(cl_root_normalized) {
                rel.trim_start_matches('/').to_string()
            } else {
                full_path.to_string()
            }
        })
        .collect();

    Ok(file_paths)
}

/// Collect Cedar policy files from directories of all changed files.
/// Also collects policies from parent directories up to Monorepo root for inheritance.
/// Tries from_hash first for security, then falls back to to_hash for new directories.
/// Returns list of (policy_path, content) tuples, ordered from root to leaf.
pub async fn collect_policy_contents<T: ApiHandler>(
    handler: &T,
    cl: &mega_cl::Model,
    changed_files: &[String],
) -> Vec<(PathBuf, String)> {
    let mut all_policy_dirs: HashSet<PathBuf> = HashSet::new();

    // Always include the CL root directory
    all_policy_dirs.insert(PathBuf::new());

    // Collect ancestor directories from all changed files
    for file_path in changed_files {
        let relative_path = file_path.trim_start_matches('/').replace('\\', "/");
        let path = PathBuf::from(&relative_path);

        let parent = path.parent().unwrap_or(std::path::Path::new(""));

        // Skip .cedar directory itself, use its parent
        let logical_parent = if parent.file_name().map(|n| n == ".cedar").unwrap_or(false) {
            parent.parent().unwrap_or(std::path::Path::new(""))
        } else {
            parent
        };

        for ancestor in logical_parent.ancestors() {
            let ancestor_str = ancestor.to_string_lossy();
            if ancestor_str.contains(".cedar") {
                continue;
            }
            let normalized = PathBuf::from(ancestor_str.replace('\\', "/"));
            all_policy_dirs.insert(normalized);
        }
    }

    // Sort by depth for correct override semantics (root policies first)
    let mut sorted_dirs: Vec<PathBuf> = all_policy_dirs.into_iter().collect();
    sorted_dirs.sort_by_key(|p| p.components().count());

    let mut policy_contents: Vec<(PathBuf, String)> = Vec::new();
    let mut seen_policies: HashSet<String> = HashSet::new();

    let self_path_str = cl.path.to_string().replace('\\', "/");
    let self_path_normalized = self_path_str.trim_start_matches('/');

    // Step 1: Collect parent policies from Monorepo root down to CL directory
    // This enables inheritance from e.g. /project/.cedar/policies.cedar
    let parent_dirs = collect_parent_policy_dirs(cl);

    for parent_dir in parent_dirs {
        // Use absolute path as key to avoid collision with CL-local policies
        let absolute_policy_path = if parent_dir.is_empty() {
            "/.cedar/policies.cedar".to_string()
        } else {
            format!("/{}/.cedar/policies.cedar", parent_dir)
        };

        if seen_policies.contains(&absolute_policy_path) {
            continue;
        }

        // For parent policies, we use a rooted MonoApiService
        if let Some(content) = get_parent_policy_content(handler, &parent_dir).await {
            seen_policies.insert(absolute_policy_path.clone());
            policy_contents.push((PathBuf::from(&absolute_policy_path), content));
        }
    }

    // Step 2: Collect policies within the CL directory
    for dir in sorted_dirs {
        let policy_relative_path = if dir.as_os_str().is_empty() {
            ".cedar/policies.cedar".to_string()
        } else {
            let dir_str = dir.to_string_lossy().replace('\\', "/");
            format!("{}/.cedar/policies.cedar", dir_str)
        };

        // Build absolute path for deduplication
        let absolute_policy_path = if self_path_normalized.is_empty() {
            format!("/{}", policy_relative_path)
        } else {
            format!("/{}/{}", self_path_normalized, policy_relative_path)
        };

        // Skip if already seen from parent collection
        if seen_policies.contains(&absolute_policy_path) {
            continue;
        }

        let lookup_path = PathBuf::from(&policy_relative_path);

        // Fetch policy content: try from_hash for existing, fall back to to_hash for new
        let content = if cl.from_hash != ZERO_ID {
            if let Ok(Some(content)) = handler
                .get_blob_as_string(lookup_path.clone(), Some(&cl.from_hash))
                .await
            {
                Some(content)
            } else {
                handler
                    .get_blob_as_string(lookup_path, Some(&cl.to_hash))
                    .await
                    .ok()
                    .flatten()
            }
        } else {
            handler
                .get_blob_as_string(lookup_path, Some(&cl.to_hash))
                .await
                .ok()
                .flatten()
        };

        if let Some(content) = content {
            seen_policies.insert(absolute_policy_path.clone());
            policy_contents.push((PathBuf::from(&absolute_policy_path), content));
        }
    }

    policy_contents
}

/// Collect parent directory paths from Monorepo root to CL directory (exclusive).
pub fn collect_parent_policy_dirs(cl: &mega_cl::Model) -> Vec<String> {
    let self_path_str = cl.path.to_string().replace('\\', "/");
    let self_path_normalized = self_path_str.trim_start_matches('/');

    if self_path_normalized.is_empty() {
        return vec![];
    }

    let mut parent_dirs = Vec::new();
    let components: Vec<&str> = self_path_normalized.split('/').collect();

    // Add root directory
    parent_dirs.push(String::new());

    // Add each parent level except the CL directory itself
    let mut current_path = String::new();
    for (i, component) in components.iter().enumerate() {
        if i == components.len() - 1 {
            break;
        }
        if current_path.is_empty() {
            current_path = component.to_string();
        } else {
            current_path = format!("{}/{}", current_path, component);
        }
        parent_dirs.push(current_path.clone());
    }

    parent_dirs
}

/// Get policy content from a parent directory using storage directly.
pub async fn get_parent_policy_content<T: ApiHandler>(
    handler: &T,
    parent_dir: &str,
) -> Option<String> {
    let storage = handler.get_context();
    let mono_storage = storage.mono_storage();

    // Get the main ref for the parent directory
    let parent_path = if parent_dir.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parent_dir)
    };

    let refs = mono_storage.get_main_ref(&parent_path).await.ok()??;

    // Look up .cedar/policies.cedar in the parent directory
    let policy_path = PathBuf::from(".cedar/policies.cedar");
    handler
        .get_blob_as_string(policy_path, Some(&refs.ref_commit_hash))
        .await
        .ok()
        .flatten()
}

pub async fn create_repo_commit(storage: &Storage, repo_path: &str) -> Result<String, MegaError> {
    let storage = storage.mono_storage();

    let path_refs = storage.get_all_refs(repo_path, false).await?;

    let heads_exist = path_refs
        .iter()
        .any(|x| x.ref_name == common::utils::MEGA_BRANCH_NAME);

    let refs: Vec<mega_refs::Model> = if heads_exist {
        path_refs
    } else {
        let target_path = PathBuf::from(repo_path);
        let mut refs: Vec<_> = vec![];

        let root_refs = storage.get_all_refs("/", true).await?;

        for root_ref in root_refs {
            let (tree_hash, commit_hash) = (root_ref.ref_tree_hash, root_ref.ref_commit_hash);
            let mut tree: Tree =
                Tree::from_mega_model(storage.get_tree_by_hash(&tree_hash).await?.unwrap());

            let commit: Commit = Commit::from_mega_model(
                storage
                    .get_commit_by_hash(&commit_hash)
                    .await?
                    .expect("can't get commit by ref.ref_commit_hash"),
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
                                .await?
                                .expect("can't get commit by tree_items hash"),
                        );
                    } else {
                        return Ok(ZERO_ID.to_string());
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
                repo_path,
                root_ref.ref_name.clone(),
                c.id.to_string(),
                c.tree_id.to_string(),
                false,
            );

            storage
                .mega_head_hash_with_txn(new_mega_ref.clone(), c)
                .await?;

            refs.push(new_mega_ref);
        }
        refs
    };
    let mut head_hash = ZERO_ID.to_string();
    for git_ref in refs.iter() {
        if git_ref.ref_name == common::utils::MEGA_BRANCH_NAME {
            head_hash.clone_from(&git_ref.ref_commit_hash);
        }
    }
    Ok(head_hash)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use git_internal::{
        hash::ObjectHash,
        internal::object::tree::{Tree, TreeItem, TreeItemMode},
    };

    use super::{has_buck_root_markers, resolve_repo_root_from_repo_paths};

    #[test]
    fn test_resolve_repo_root_from_repo_paths_prefers_longest_registered_prefix() {
        let repo_paths = vec![
            "/project".to_string(),
            "/project/buck2_test".to_string(),
            "/project/buck2_test_extra".to_string(),
        ];

        assert_eq!(
            resolve_repo_root_from_repo_paths("/project/buck2_test/src", &repo_paths).unwrap(),
            Some("/project/buck2_test".to_string())
        );
    }

    #[test]
    fn test_resolve_repo_root_from_repo_paths_requires_path_boundary() {
        let repo_paths = vec!["/project/buck2_test".to_string()];

        assert_eq!(
            resolve_repo_root_from_repo_paths("/project/buck2_test_extra/src", &repo_paths)
                .unwrap(),
            None
        );
    }

    #[test]
    fn test_resolve_repo_root_from_repo_paths_normalizes_inputs() {
        let repo_paths = vec!["project/buck2_test".to_string()];

        assert_eq!(
            resolve_repo_root_from_repo_paths("project/buck2_test/src", &repo_paths).unwrap(),
            Some("/project/buck2_test".to_string())
        );
    }

    #[test]
    fn test_has_buck_root_markers_requires_buck_files_not_toolchains_dir() {
        let buckroot = TreeItem {
            mode: TreeItemMode::Blob,
            id: ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap(),
            name: ".buckroot".to_string(),
        };
        let buckconfig = TreeItem {
            mode: TreeItemMode::Blob,
            id: ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap(),
            name: ".buckconfig".to_string(),
        };
        let src_dir = TreeItem {
            mode: TreeItemMode::Tree,
            id: ObjectHash::from_str("3333333333333333333333333333333333333333").unwrap(),
            name: "src".to_string(),
        };
        let toolchains_dir = TreeItem {
            mode: TreeItemMode::Tree,
            id: ObjectHash::from_str("4444444444444444444444444444444444444444").unwrap(),
            name: "toolchains".to_string(),
        };

        let root_tree =
            Tree::from_tree_items(vec![buckroot.clone(), buckconfig.clone(), src_dir.clone()])
                .unwrap();
        assert!(has_buck_root_markers(&root_tree));

        let root_with_toolchains_only =
            Tree::from_tree_items(vec![src_dir.clone(), toolchains_dir]).unwrap();
        assert!(
            !has_buck_root_markers(&root_with_toolchains_only),
            "toolchains/ alone must not mark a Buck root"
        );

        let subtree = Tree::from_tree_items(vec![src_dir]).unwrap();
        assert!(!has_buck_root_markers(&subtree));

        let partial = Tree::from_tree_items(vec![buckroot]).unwrap();
        assert!(!has_buck_root_markers(&partial));
    }
}
