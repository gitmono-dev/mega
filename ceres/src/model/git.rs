use std::collections::HashMap;

use api_model::git::commit::LatestCommitInfo;
use git_internal::internal::object::{
    commit::Commit,
    tree::{TreeItem, TreeItemMode},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(PartialEq, Eq, Debug, Clone, Deserialize, ToSchema)]
pub struct CreateEntryInfo {
    /// can be a file or directory
    pub is_directory: bool,
    pub name: String,
    /// leave empty if it's under root
    pub path: String,
    // pub import_dir: bool,
    pub content: Option<String>,
    /// web user email for commit binding
    pub author_email: Option<String>,
    /// web username for commit binding (optional)
    pub author_username: Option<String>,
    /// if true, skip build
    #[serde(default)]
    pub skip_build: bool,
    /// Controls how CL is created or reused for this change.
    #[serde(default = "default_create_mode")]
    pub mode: EditCLMode,
}

impl CreateEntryInfo {
    pub fn commit_msg(&self) -> String {
        if self.is_directory {
            format!("create new directory {}", self.name)
        } else {
            format!("create new file {}", self.name)
        }
    }
}

fn default_create_mode() -> EditCLMode {
    EditCLMode::TryReuse(None)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct CodePreviewQuery {
    #[serde(default)]
    pub refs: String,
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TreeQuery {
    pub oid: Option<String>,
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct BlobContentQuery {
    #[serde(default)]
    pub refs: String,
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CommitBindingInfo {
    pub matched_username: Option<String>,
    pub is_anonymous: bool,
}

pub struct LatestCommitInfoWrapper(pub LatestCommitInfo);

impl From<Commit> for LatestCommitInfoWrapper {
    fn from(commit: Commit) -> Self {
        let message = commit.format_message();
        Self(LatestCommitInfo {
            oid: commit.id.to_string(),
            date: commit.committer.timestamp.to_string(),
            short_message: message,
            author: commit.author.name,
            committer: commit.committer.name,
            status: "success".to_string(),
        })
    }
}

// UserInfo removed: author/committer are now plain strings

#[derive(Serialize, Deserialize, ToSchema)]
pub struct TreeCommitItem {
    pub commit_id: String,
    pub name: String,
    pub content_type: String,
    pub commit_message: String,
    pub date: String,
}

impl From<(TreeItem, Option<Commit>)> for TreeCommitItem {
    fn from((item, commit): (TreeItem, Option<Commit>)) -> Self {
        TreeCommitItem {
            name: item.name.clone(),
            content_type: if item.mode == TreeItemMode::Tree {
                "directory".to_owned()
            } else {
                "file".to_owned()
            },
            commit_id: commit
                .as_ref()
                .map(|x| x.id.to_string())
                .unwrap_or_default(),
            commit_message: commit
                .as_ref()
                .map(|x| x.format_message())
                .unwrap_or_default(),
            date: commit
                .as_ref()
                .map(|x| x.committer.timestamp.to_string())
                .unwrap_or_default(),
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct TreeHashItem {
    pub name: String,
    pub content_type: String,
    pub oid: String,
}

impl From<TreeItem> for TreeHashItem {
    fn from(value: TreeItem) -> Self {
        Self {
            oid: value.id.to_string(),
            name: value.name,
            content_type: if value.mode == TreeItemMode::Tree {
                "directory".to_owned()
            } else {
                "file".to_owned()
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreeBriefItem {
    pub name: String,
    pub path: String,
    pub content_type: String,
}

impl From<TreeItem> for TreeBriefItem {
    fn from(value: TreeItem) -> Self {
        TreeBriefItem {
            name: value.name,
            path: String::new(),
            content_type: if value.mode == TreeItemMode::Tree {
                "directory".to_owned()
            } else {
                "file".to_owned()
            },
        }
    }
}

fn default_path() -> String {
    "/".to_string()
}

/// Request body for previewing diff of a single file before saving.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiffPreviewPayload {
    /// Full file path like "/project/dir/file.rs"
    pub path: String,
    /// New content to preview against current HEAD
    pub content: String,
    /// Optional refs (commit SHA or tag); empty/default means current HEAD
    #[serde(default)]
    pub refs: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditCLMode {
    /// force create new cl
    ForceCreate,
    /// try to reuse old cl, if none, will search existing open cl, and create new cl if not found
    TryReuse(Option<String>),
}
/// Request body for saving an edited file with conflict detection.
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct EditFilePayload {
    /// Full file path like "/project/dir/file.rs"
    pub path: String,
    /// Optional destination path for same-directory rename (e.g. "/project/dir/renamed.rs").
    /// Must share the same parent directory as `path`.
    #[serde(default)]
    pub new_path: Option<String>,
    /// New file content to save
    pub content: String,
    /// Commit message to use when creating the commit
    pub commit_message: String,
    /// author email to bind this commit to a user
    #[serde(default)]
    pub author_email: Option<String>,
    /// platform username (used to verify and bind commit to user)
    #[serde(default)]
    pub author_username: Option<String>,
    /// if true, skip build
    #[serde(default)]
    pub skip_build: bool,
    #[serde(default = "default_edit_mode")]
    pub mode: EditCLMode,
}

fn default_edit_mode() -> EditCLMode {
    EditCLMode::TryReuse(None)
}

/// Response body after saving an edited file
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EditFileResult {
    /// New commit id created by this save
    pub commit_id: String,
    /// New blob oid of the saved file
    pub new_oid: String,
    /// Saved file path
    pub path: String,
    pub cl_link: Option<String>,
}

/// Response body after creating a file or directory
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateEntryResult {
    /// New commit id created by this operation
    pub commit_id: String,
    /// New blob oid for the created entry
    pub new_oid: String,
    /// Created entry path
    pub path: String,
    pub cl_link: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreeResponse {
    pub file_tree: HashMap<String, FileTreeItem>,
    pub tree_items: Vec<TreeBriefItem>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FileTreeItem {
    pub tree_items: Vec<TreeBriefItem>,
    pub total_count: usize,
}
