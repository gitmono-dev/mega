use std::path::PathBuf;

use api_model::common::CommonPage;
use callisto::{check_result, sea_orm_active_enums::MergeStatusEnum};
use git_internal::{DiffItem, hash::ObjectHash};
use jupiter::model::{cl_dto::CLDetails, common::ListParams};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    merge_checker::{CheckType, ConditionResult},
    model::{conversation::ConversationItem, label::LabelItem},
};

#[derive(Deserialize, ToSchema)]
pub struct AssigneeUpdatePayload {
    pub assignees: Vec<String>,
    pub item_id: i64,
    pub link: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ListPayload {
    pub status: String,
    pub author: Option<String>,
    pub labels: Option<Vec<i64>>,
    pub assignees: Option<Vec<String>>,
    pub sort_by: Option<String>,
    pub asc: bool,
}

impl From<ListPayload> for ListParams {
    fn from(value: ListPayload) -> Self {
        Self {
            status: value.status,
            author: value.author,
            labels: value.labels,
            assignees: value.assignees,
            sort_by: value.sort_by,
            asc: value.asc,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CLDetailRes {
    pub id: i64,
    pub link: String,
    pub title: String,
    pub status: MergeStatus,
    pub open_timestamp: i64,
    pub merge_timestamp: Option<i64>,
    pub conversations: Vec<ConversationItem>,
    pub labels: Vec<LabelItem>,
    pub assignees: Vec<String>,
    pub path: String,
}

impl From<CLDetails> for CLDetailRes {
    fn from(value: CLDetails) -> Self {
        Self {
            id: value.cl.id,
            link: value.cl.link,
            title: value.cl.title,
            status: value.cl.status.into(),
            open_timestamp: value.cl.created_at.and_utc().timestamp(),
            merge_timestamp: value.cl.merge_date.map(|dt| dt.and_utc().timestamp()),
            conversations: value
                .conversations
                .into_iter()
                .map(|x| ConversationItem::from_model(x.conversation, x.reactions, &value.username))
                .collect(),
            labels: value.labels.into_iter().map(|x| x.into()).collect(),
            assignees: value
                .assignees
                .into_iter()
                .map(|x| x.assignnee_id)
                .collect(),
            path: value.cl.path,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct FilesChangedPage {
    pub page: CommonPage<ClFilesChangedItemSchema>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct ClFilesChangedItemSchema {
    pub path: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct DiffItemSchema {
    pub path: String,
    pub data: String,
}

impl From<DiffItem> for DiffItemSchema {
    fn from(item: DiffItem) -> Self {
        Self {
            path: item.path,
            data: item.data,
        }
    }
}

impl ClFilesChangedItemSchema {
    pub fn new(item: DiffItem, old_path: Option<String>) -> Self {
        Self {
            path: item.path,
            data: item.data,
            old_path,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct UpdateBranchStatusRes {
    pub base_commit: String,
    pub target_head: String,
    pub outdated: bool,
}

#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct MuiTreeNode {
    id: String,
    pub label: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(no_recursion)]
    pub children: Option<Vec<MuiTreeNode>>,
}

impl MuiTreeNode {
    pub fn new(label: &str, path: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            label: label.to_string(),
            path: path.to_string(),
            children: None,
        }
    }

    pub fn insert_path(&mut self, parts: &[&str], buf: &mut String) {
        if parts.is_empty() {
            return;
        }

        if self.children.is_none() {
            self.children = Some(Vec::new());
        }

        let children = self.children.as_mut().unwrap();

        if let Some(existing) = children.iter_mut().find(|c| c.label == parts[0]) {
            let mut buf = existing.path.clone();
            existing.insert_path(&parts[1..], &mut buf);
        } else {
            buf.push('/');
            buf.push_str(parts[0]);
            let mut new_node = MuiTreeNode::new(parts[0], buf);
            new_node.insert_path(&parts[1..], buf);
            children.push(new_node);
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ClFilesRes {
    pub path: String,
    pub sha: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<u8>,
}

impl From<ClDiffFile> for ClFilesRes {
    fn from(value: ClDiffFile) -> Self {
        match value {
            ClDiffFile::New(path, sha) => Self {
                path: path.to_string_lossy().replace('\\', "/"),
                sha: sha.to_string(),
                action: "new".to_owned(),
                old_path: None,
                similarity: None,
            },
            ClDiffFile::Deleted(path, sha) => Self {
                path: path.to_string_lossy().replace('\\', "/"),
                sha: sha.to_string(),
                action: "deleted".to_owned(),
                old_path: None,
                similarity: None,
            },
            ClDiffFile::Modified(path, _, new) => Self {
                path: path.to_string_lossy().replace('\\', "/"),
                sha: new.to_string(),
                action: "modified".to_owned(),
                old_path: None,
                similarity: None,
            },
            ClDiffFile::Renamed(old_path, new_path, _, new_hash, similarity)
            | ClDiffFile::Moved(old_path, new_path, _, new_hash, similarity) => Self {
                path: new_path.to_string_lossy().replace('\\', "/"),
                sha: new_hash.to_string(),
                action: "renamed".to_owned(),
                old_path: Some(old_path.to_string_lossy().replace('\\', "/")),
                similarity: Some(similarity),
            },
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub enum MergeStatus {
    Open,
    Merged,
    Closed,
    Draft,
}

impl From<MergeStatusEnum> for MergeStatus {
    fn from(value: MergeStatusEnum) -> Self {
        match value {
            MergeStatusEnum::Open => MergeStatus::Open,
            MergeStatusEnum::Merged => MergeStatus::Merged,
            MergeStatusEnum::Closed => MergeStatus::Closed,
            MergeStatusEnum::Draft => MergeStatus::Draft,
        }
    }
}

impl From<MergeStatus> for MergeStatusEnum {
    fn from(value: MergeStatus) -> Self {
        match value {
            MergeStatus::Open => MergeStatusEnum::Open,
            MergeStatus::Merged => MergeStatusEnum::Merged,
            MergeStatus::Closed => MergeStatusEnum::Closed,
            MergeStatus::Draft => MergeStatusEnum::Draft,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct MergeBoxRes {
    pub merge_requirements: Option<MergeRequirements>,
}

impl MergeBoxRes {
    pub fn from_condition(conditions: Vec<Condition>) -> Self {
        let mut state = RequirementsState::MERGEABLE;
        for cond in &conditions {
            // CLA is advisory-only: keep reporting the result, but do not mark the CL unmergeable.
            if cond.result == ConditionResult::FAILED
                && cond.condition_type != CheckType::ClaSign
            {
                state = RequirementsState::UNMERGEABLE
            }
        }
        MergeBoxRes {
            merge_requirements: Some(MergeRequirements { conditions, state }),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct MergeRequirements {
    pub conditions: Vec<Condition>,
    pub state: RequirementsState,
}

#[derive(Serialize, ToSchema)]
pub struct Condition {
    #[serde(rename = "type")]
    pub condition_type: CheckType,
    pub display_name: String,
    pub description: String,
    pub message: String,
    pub result: ConditionResult,
}

impl From<check_result::Model> for Condition {
    fn from(value: check_result::Model) -> Self {
        let check_type: CheckType = value.check_type_code.into();
        Self {
            condition_type: check_type.clone(),
            display_name: check_type.clone().display_name().to_string(),
            description: check_type.description().to_string(),
            message: value.message,
            result: value.status.parse().unwrap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub enum RequirementsState {
    UNMERGEABLE,
    MERGEABLE,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]

pub struct VerifyClPayload {
    pub assignees: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct UpdateClStatusPayload {
    pub status: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ReviewerPayload {
    pub reviewer_usernames: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ReviewersResponse {
    pub result: Vec<ReviewerInfo>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ChangeReviewerStatePayload {
    pub approved: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ChangeReviewStatePayload {
    pub conversation_id: i64,
    pub resolved: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct SetSystemReviewersPayload {
    pub target_reviewer_usernames: Vec<String>,
    pub is_system_required: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ReviewerInfo {
    pub username: String,
    pub approved: bool,
    pub system_required: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct CloneRepoPayload {
    pub owner: String,
    pub repo: String,
    pub path: String,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub enum ClDiffFile {
    New(PathBuf, ObjectHash),
    Deleted(PathBuf, ObjectHash),
    // path, old_hash, new_hash
    Modified(PathBuf, ObjectHash, ObjectHash),
    // old_path, new_path, old_hash, new_hash, similarity
    Renamed(PathBuf, PathBuf, ObjectHash, ObjectHash, u8),
    // old_path, new_path, old_hash, new_hash, similarity
    Moved(PathBuf, PathBuf, ObjectHash, ObjectHash, u8),
}

impl ClDiffFile {
    pub fn path(&self) -> &PathBuf {
        match self {
            ClDiffFile::New(path, _) => path,
            ClDiffFile::Deleted(path, _) => path,
            ClDiffFile::Modified(path, _, _) => path,
            ClDiffFile::Renamed(_, new_path, _, _, _) => new_path,
            ClDiffFile::Moved(_, new_path, _, _, _) => new_path,
        }
    }

    pub fn kind_weight(&self) -> u8 {
        match self {
            ClDiffFile::New(_, _) => 0,
            ClDiffFile::Deleted(_, _) => 1,
            ClDiffFile::Renamed(_, _, _, _, _) => 2,
            ClDiffFile::Moved(_, _, _, _, _) => 3,
            ClDiffFile::Modified(_, _, _) => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use git_internal::{DiffItem, hash::ObjectHash};
    use serde_json::Value;

    use super::{ClDiffFile, ClFilesChangedItemSchema, ClFilesRes};

    #[test]
    fn relocated_files_serialize_as_renamed() {
        let old_hash = ObjectHash::from_str("1111111111111111111111111111111111111111").unwrap();
        let new_hash = ObjectHash::from_str("2222222222222222222222222222222222222222").unwrap();

        let renamed = ClFilesRes::from(ClDiffFile::Renamed(
            PathBuf::from("old_dir/file.rs"),
            PathBuf::from("new_dir/file.rs"),
            old_hash,
            new_hash,
            91,
        ));
        let moved = ClFilesRes::from(ClDiffFile::Moved(
            PathBuf::from("old_dir/file.rs"),
            PathBuf::from("other_dir/file.rs"),
            old_hash,
            new_hash,
            88,
        ));

        let renamed_json: Value = serde_json::to_value(renamed).unwrap();
        let moved_json: Value = serde_json::to_value(moved).unwrap();

        assert_eq!(renamed_json["action"], "renamed");
        assert_eq!(renamed_json["old_path"], "old_dir/file.rs");
        assert_eq!(renamed_json["similarity"], 91);
        assert!(renamed_json.get("display_action").is_none());

        assert_eq!(moved_json["action"], "renamed");
        assert_eq!(moved_json["old_path"], "old_dir/file.rs");
        assert_eq!(moved_json["similarity"], 88);
        assert!(moved_json.get("display_action").is_none());
    }

    #[test]
    fn cl_files_changed_item_schema_omits_old_path_for_non_relocated_items() {
        let regular = ClFilesChangedItemSchema::new(
            DiffItem {
                path: "dir/file.rs".to_string(),
                data: "diff --git a/dir/file.rs b/dir/file.rs\n".to_string(),
            },
            None,
        );
        let renamed = ClFilesChangedItemSchema::new(
            DiffItem {
                path: "new_dir/file.rs".to_string(),
                data: "diff --git a/old_dir/file.rs b/new_dir/file.rs\n".to_string(),
            },
            Some("old_dir/file.rs".to_string()),
        );

        let regular_json: Value = serde_json::to_value(regular).unwrap();
        let renamed_json: Value = serde_json::to_value(renamed).unwrap();

        assert!(regular_json.get("old_path").is_none());
        assert_eq!(renamed_json["old_path"], "old_dir/file.rs");
    }
}
