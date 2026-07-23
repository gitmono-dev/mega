use std::path::{Component, Path, PathBuf};
pub use std::sync::Arc;

use api_model::buck2::{status::Status, types::ProjectRelativePath};
use common::errors::MegaError;
use git_internal::hash::ObjectHash;

use super::changes_port::ChangesPort;
use crate::{application::build_trigger::TriggerContext, model::change_list::ClDiffFile};

fn is_safe_normalized_path(path: &str) -> bool {
    path.is_empty()
        || (!path.contains("//")
            && Path::new(path)
                .components()
                .all(|component| matches!(component, Component::Normal(_))))
}

fn normalize_change_path_for_repo_with_prefix(
    repo_prefix: &str,
    repo_prefix_with_slash: Option<&str>,
    path: &Path,
) -> Option<ProjectRelativePath> {
    let raw = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();

    let normalized = if repo_prefix.is_empty() {
        raw
    } else if raw == repo_prefix {
        String::new()
    } else if let Some(prefix) = repo_prefix_with_slash {
        if let Some(stripped) = raw.strip_prefix(prefix) {
            stripped.to_string()
        } else {
            raw
        }
    } else {
        raw
    };

    if !is_safe_normalized_path(&normalized) {
        tracing::warn!(
            path = %normalized,
            "Dropping unsafe build change path after normalization."
        );
        return None;
    }

    Some(ProjectRelativePath::new(&normalized))
}

fn detect_single_level_prefixed_candidate(repo_root: &Path, normalized: &str) -> Option<String> {
    if normalized.is_empty() || normalized.contains('/') {
        return None;
    }
    if !repo_root.exists() || repo_root.join(normalized).exists() {
        return None;
    }

    let mut candidate: Option<String> = None;
    let entries = std::fs::read_dir(repo_root).ok()?;
    for entry in entries.flatten() {
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let dir_name = entry.file_name();
        let dir_name = dir_name.to_str()?;
        let prefixed = format!("{dir_name}/{normalized}");
        if !repo_root.join(&prefixed).exists() {
            continue;
        }

        if candidate.is_some() {
            return None;
        }
        candidate = Some(prefixed);
    }

    candidate
}

fn monitor_possible_repo_prefix_mismatch(repo_path: &str, raw: &Path, normalized: &str) {
    let repo_root = Path::new(repo_path);
    if !repo_root.exists() || normalized.is_empty() || repo_root.join(normalized).exists() {
        return;
    }

    let Some(prefixed_candidate) = detect_single_level_prefixed_candidate(repo_root, normalized)
    else {
        return;
    };

    tracing::warn!(
        monitor_event = "build_change_path_prefix_mismatch",
        repo_path = %repo_path,
        raw_path = %raw.display(),
        normalized_path = %normalized,
        suggested_path = %prefixed_candidate,
        "Detected possible change-path prefix drift after normalization."
    );
}

#[cfg(test)]
fn normalize_change_path_for_repo(repo_path: &str, path: &Path) -> Option<ProjectRelativePath> {
    let repo_prefix = repo_path.trim_matches('/');
    let repo_prefix_with_slash = (!repo_prefix.is_empty()).then(|| format!("{repo_prefix}/"));
    normalize_change_path_for_repo_with_prefix(repo_prefix, repo_prefix_with_slash.as_deref(), path)
}

fn push_change_if_valid(
    changes: &mut Vec<Status<ProjectRelativePath>>,
    status_builder: impl FnOnce(ProjectRelativePath) -> Status<ProjectRelativePath>,
    normalized: Option<ProjectRelativePath>,
) {
    if let Some(path) = normalized {
        changes.push(status_builder(path));
    }
}

/// Rebase a CL-relative file path onto the Buck repo root when the CL directory
/// is a strict subdirectory of `repo_path`.
///
/// Example: repo=`/`, cl=`/project/dagrs-derive`, file=`src/lib.rs`
/// → `project/dagrs-derive/src/lib.rs`
fn rebase_cl_relative_path(repo_path: &str, cl_path: &str, file: &Path) -> PathBuf {
    let repo = repo_path.trim_matches('/');
    let cl = cl_path.trim_matches('/');
    let file_str = file
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();

    let cl_rel = if repo.is_empty() {
        if cl.is_empty() {
            return PathBuf::from(&file_str);
        }
        cl.to_string()
    } else if cl == repo {
        return PathBuf::from(&file_str);
    } else if let Some(stripped) = cl.strip_prefix(&format!("{repo}/")) {
        stripped.to_string()
    } else {
        return PathBuf::from(&file_str);
    };

    if cl_rel.is_empty() {
        return PathBuf::from(&file_str);
    }

    if file_str == cl_rel || file_str.starts_with(&format!("{cl_rel}/")) {
        return PathBuf::from(&file_str);
    }

    PathBuf::from(format!("{cl_rel}/{file_str}"))
}

fn build_changes_for_repo(
    repo_path: &str,
    cl_path: Option<&str>,
    cl_diff_files: Vec<ClDiffFile>,
) -> Result<Vec<Status<ProjectRelativePath>>, MegaError> {
    let repo_prefix = repo_path.trim_matches('/');
    let repo_prefix_with_slash = (!repo_prefix.is_empty()).then(|| format!("{repo_prefix}/"));
    let to_project_relative = |path: &Path| {
        let rebased = match cl_path {
            Some(cl) => rebase_cl_relative_path(repo_path, cl, path),
            None => path.to_path_buf(),
        };
        let normalized = normalize_change_path_for_repo_with_prefix(
            repo_prefix,
            repo_prefix_with_slash.as_deref(),
            &rebased,
        );
        if let Some(normalized_path) = &normalized {
            monitor_possible_repo_prefix_mismatch(repo_path, &rebased, normalized_path.as_str());
        }
        normalized
    };

    let mut counter_changes = Vec::new();
    for change in cl_diff_files {
        match change {
            ClDiffFile::New(path, _) => {
                push_change_if_valid(
                    &mut counter_changes,
                    Status::Added,
                    to_project_relative(&path),
                );
            }
            ClDiffFile::Deleted(path, _) => {
                push_change_if_valid(
                    &mut counter_changes,
                    Status::Removed,
                    to_project_relative(&path),
                );
            }
            ClDiffFile::Modified(path, _, _) => {
                push_change_if_valid(
                    &mut counter_changes,
                    Status::Modified,
                    to_project_relative(&path),
                );
            }
            ClDiffFile::Renamed(old_path, new_path, _, _, _)
            | ClDiffFile::Moved(old_path, new_path, _, _, _) => {
                push_change_if_valid(
                    &mut counter_changes,
                    Status::Removed,
                    to_project_relative(&old_path),
                );
                push_change_if_valid(
                    &mut counter_changes,
                    Status::Added,
                    to_project_relative(&new_path),
                );
            }
        }
    }

    Ok(counter_changes)
}

pub struct ChangesCalculator<P: ChangesPort> {
    port: P,
}

/// Changes calculator backed by a shared [`ChangesPort`].
pub type MonoChangesCalculator = ChangesCalculator<Arc<dyn ChangesPort>>;

impl<P: ChangesPort> ChangesCalculator<P> {
    pub fn new(port: P) -> Self {
        Self { port }
    }

    pub async fn get_builds_for_commit(
        &self,
        context: &TriggerContext,
    ) -> Result<Vec<Status<ProjectRelativePath>>, MegaError> {
        let old_files = self.get_commit_blobs(&context.from_hash).await?;
        let new_files = self.get_commit_blobs(&context.commit_hash).await?;
        let diff_files = self.cl_files_list(old_files, new_files).await?;

        let changes =
            build_changes_for_repo(&context.repo_path, context.cl_path.as_deref(), diff_files)?;

        Ok(changes)
    }

    async fn get_commit_blobs(
        &self,
        commit_hash: &str,
    ) -> Result<Vec<(PathBuf, ObjectHash)>, MegaError> {
        self.port.get_commit_blobs(commit_hash).await
    }

    async fn cl_files_list(
        &self,
        old_files: Vec<(PathBuf, ObjectHash)>,
        new_files: Vec<(PathBuf, ObjectHash)>,
    ) -> Result<Vec<ClDiffFile>, MegaError> {
        self.port.cl_files_list(old_files, new_files).await
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, str::FromStr};

    use git_internal::hash::ObjectHash;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_normalize_change_path_for_repo_strips_repo_prefix_for_local_files() {
        assert_eq!(
            normalize_change_path_for_repo("/project/buck2_test", &PathBuf::from("src/main.rs")),
            Some(ProjectRelativePath::new("src/main.rs"))
        );
        assert_eq!(
            normalize_change_path_for_repo(
                "/project/buck2_test",
                &PathBuf::from("project/buck2_test/src/generated.rs")
            ),
            Some(ProjectRelativePath::new("src/generated.rs"))
        );
    }

    #[test]
    fn test_normalize_change_path_for_repo_keeps_external_shared_paths() {
        assert_eq!(
            normalize_change_path_for_repo("/project/buck2_test", &PathBuf::from("common/lib.rs")),
            Some(ProjectRelativePath::new("common/lib.rs"))
        );
    }

    #[test]
    fn test_normalize_change_path_for_repo_rejects_unsafe_paths() {
        assert_eq!(
            normalize_change_path_for_repo("/project/buck2_test", &PathBuf::from("../secret.rs")),
            None
        );
        assert_eq!(
            normalize_change_path_for_repo(
                "/project/buck2_test",
                &PathBuf::from("project/buck2_test/../../secret.rs")
            ),
            None
        );
        assert_eq!(
            normalize_change_path_for_repo(
                "/project/buck2_test",
                &PathBuf::from("project//buck2_test/src/main.rs")
            ),
            None
        );
    }

    #[test]
    fn test_build_changes_normalizes_repo_local_paths_and_keeps_external_paths() {
        let changes = build_changes_for_repo(
            "/project/buck2_test",
            Some("/project/buck2_test"),
            vec![
                ClDiffFile::Modified(
                    PathBuf::from("src/main.rs"),
                    ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap(),
                    ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("src/generated.rs"),
                    ObjectHash::from_str("3333333333333333333333333333333333333333").unwrap(),
                ),
                ClDiffFile::Deleted(
                    PathBuf::from("README.md"),
                    ObjectHash::from_str("4444444444444444444444444444444444444444").unwrap(),
                ),
                ClDiffFile::Modified(
                    PathBuf::from("common/lib.rs"),
                    ObjectHash::from_str("5555555555555555555555555555555555555555").unwrap(),
                    ObjectHash::from_str("6666666666666666666666666666666666666666").unwrap(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![
                Status::Modified(ProjectRelativePath::new("src/main.rs")),
                Status::Added(ProjectRelativePath::new("src/generated.rs")),
                Status::Removed(ProjectRelativePath::new("README.md")),
                Status::Modified(ProjectRelativePath::new("common/lib.rs")),
            ]
        );
    }

    #[test]
    fn test_build_changes_filters_unsafe_paths() {
        let changes = build_changes_for_repo(
            "/project/buck2_test",
            Some("/project/buck2_test"),
            vec![
                ClDiffFile::Modified(
                    PathBuf::from("src/main.rs"),
                    ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap(),
                    ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("../outside.rs"),
                    ObjectHash::from_str("3333333333333333333333333333333333333333").unwrap(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![Status::Modified(ProjectRelativePath::new("src/main.rs"))]
        );
    }

    #[test]
    fn test_build_changes_keeps_added_paths_for_create_entry_variants() {
        let changes = build_changes_for_repo(
            "/project/buck2_test",
            Some("/project/buck2_test"),
            vec![
                ClDiffFile::New(
                    PathBuf::from("/project/buck2_test/src/new_file_abs.rs"),
                    ObjectHash::from_str("7777777777777777777777777777777777777777").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("project/buck2_test/src/new_file_prefixed.rs"),
                    ObjectHash::from_str("8888888888888888888888888888888888888888").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("src/new_file_relative.rs"),
                    ObjectHash::from_str("9999999999999999999999999999999999999999").unwrap(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![
                Status::Added(ProjectRelativePath::new("src/new_file_abs.rs")),
                Status::Added(ProjectRelativePath::new("src/new_file_prefixed.rs")),
                Status::Added(ProjectRelativePath::new("src/new_file_relative.rs")),
            ]
        );
    }

    #[test]
    fn test_build_changes_keeps_added_gitkeep_for_new_directory() {
        let changes = build_changes_for_repo(
            "/project/buck2_test",
            Some("/project/buck2_test"),
            vec![ClDiffFile::New(
                PathBuf::from("project/buck2_test/src/new_dir/.gitkeep"),
                ObjectHash::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
            )],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![Status::Added(ProjectRelativePath::new(
                "src/new_dir/.gitkeep"
            ))]
        );
    }

    #[test]
    fn test_rebase_cl_relative_path_prefixes_monorepo_subdir_cl() {
        assert_eq!(
            rebase_cl_relative_path("/", "/project/dagrs-derive", Path::new("src/lib.rs")),
            PathBuf::from("project/dagrs-derive/src/lib.rs")
        );
        assert_eq!(
            rebase_cl_relative_path(
                "/",
                "/project/dagrs-derive",
                Path::new("project/dagrs-derive/BUCK")
            ),
            PathBuf::from("project/dagrs-derive/BUCK")
        );
    }

    #[test]
    fn test_rebase_cl_relative_path_noop_when_cl_equals_repo() {
        assert_eq!(
            rebase_cl_relative_path(
                "/project/buck2_test",
                "/project/buck2_test",
                Path::new("src/main.rs")
            ),
            PathBuf::from("src/main.rs")
        );
    }

    #[test]
    fn test_rebase_cl_relative_path_nested_cl_under_registered_repo() {
        assert_eq!(
            rebase_cl_relative_path(
                "/project/buck2_test",
                "/project/buck2_test/src",
                Path::new("main.rs")
            ),
            PathBuf::from("src/main.rs")
        );
    }

    #[test]
    fn test_build_changes_rebases_dagrs_derive_style_paths_onto_monorepo_root() {
        let changes = build_changes_for_repo(
            "/",
            Some("/project/dagrs-derive"),
            vec![
                ClDiffFile::Modified(
                    PathBuf::from("src/lib.rs"),
                    ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap(),
                    ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("BUCK"),
                    ObjectHash::from_str("3333333333333333333333333333333333333333").unwrap(),
                ),
                ClDiffFile::New(
                    PathBuf::from("project/dagrs-derive/Cargo.toml"),
                    ObjectHash::from_str("4444444444444444444444444444444444444444").unwrap(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![
                Status::Modified(ProjectRelativePath::new("project/dagrs-derive/src/lib.rs")),
                Status::Added(ProjectRelativePath::new("project/dagrs-derive/BUCK")),
                Status::Added(ProjectRelativePath::new("project/dagrs-derive/Cargo.toml")),
            ]
        );
    }

    #[test]
    fn test_build_changes_rebases_nested_cl_under_buck2_test() {
        let changes = build_changes_for_repo(
            "/project/buck2_test",
            Some("/project/buck2_test/src"),
            vec![ClDiffFile::Modified(
                PathBuf::from("main.rs"),
                ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap(),
                ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap(),
            )],
        )
        .unwrap();

        assert_eq!(
            changes,
            vec![Status::Modified(ProjectRelativePath::new("src/main.rs"))]
        );
    }

    #[test]
    fn test_detect_single_level_prefixed_candidate_returns_unique_match() {
        let tempdir = TempDir::new().expect("create tempdir");
        let repo_root = tempdir.path();
        fs::create_dir_all(repo_root.join("src")).expect("create src dir");
        fs::write(repo_root.join("src/main.rs"), "fn main() {}\n").expect("write source file");

        assert_eq!(
            detect_single_level_prefixed_candidate(repo_root, "main.rs"),
            Some("src/main.rs".to_string())
        );
    }

    #[test]
    fn test_detect_single_level_prefixed_candidate_rejects_ambiguous_matches() {
        let tempdir = TempDir::new().expect("create tempdir");
        let repo_root = tempdir.path();
        fs::create_dir_all(repo_root.join("src")).expect("create src dir");
        fs::create_dir_all(repo_root.join("examples")).expect("create examples dir");
        fs::write(repo_root.join("src/main.rs"), "fn src() {}\n").expect("write src file");
        fs::write(repo_root.join("examples/main.rs"), "fn ex() {}\n").expect("write examples file");

        assert_eq!(
            detect_single_level_prefixed_candidate(repo_root, "main.rs"),
            None
        );
    }

    #[test]
    fn test_detect_single_level_prefixed_candidate_returns_none_when_direct_path_exists() {
        let tempdir = TempDir::new().expect("create tempdir");
        let repo_root = tempdir.path();
        fs::create_dir_all(repo_root.join("src")).expect("create src dir");
        fs::write(repo_root.join("main.rs"), "fn root() {}\n").expect("write root file");
        fs::write(repo_root.join("src/main.rs"), "fn nested() {}\n").expect("write nested file");

        assert_eq!(
            detect_single_level_prefixed_candidate(repo_root, "main.rs"),
            None
        );
    }
}
