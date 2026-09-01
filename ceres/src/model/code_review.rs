use callisto::{
    mega_code_review_thread,
    sea_orm_active_enums::{DiffSideEnum, PositionStatusEnum, ThreadStatusEnum},
};
use jupiter::model::code_review_dto::{
    AnchorView, CodeReviewViews, CommentReviewView, FileReviewView, PositionView, ThreadReviewView,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::serde_snowflake::{
    deserialize_i64_from_string_or_number, deserialize_option_i64_from_string_or_number,
    serialize_i64_as_string, serialize_option_i64_as_string,
};

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct InitializeCommentRequest {
    pub file_path: String,
    pub anchor_commit_sha: String,
    pub original_line_number: i32,
    pub normalized_content: String,
    pub context_before: String,
    pub context_after: String,
    pub diff_side: DiffSide,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CommentReplyRequest {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub parent_comment_id: i64,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct UpdateCommentRequest {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum DiffSide {
    Deletions,
    Additions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ThreadStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum PositionStatus {
    Exact,
    Shifted,
    PendingReanchor,
    Ambiguous,
    NotFound,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CodeReviewResponse {
    pub link: String,
    pub files: Vec<FileReviewResponse>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct FileReviewResponse {
    pub file_path: String,
    pub threads: Vec<ThreadReviewResponse>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ThreadReviewResponse {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub thread_id: i64,
    pub status: ThreadStatus,
    pub created_at: String,
    pub updated_at: String,
    pub anchor: AnchorResponse,
    pub position: PositionResponse,
    pub comments: Vec<CommentReviewResponse>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ThreadStatusResponse {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub thread_id: i64,
    pub link: String,
    pub status: ThreadStatus,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct AnchorResponse {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub anchor_id: i64,
    pub file_path: String,
    pub diff_side: DiffSide,
    pub anchor_commit_sha: String,
    pub original_line_number: i32,
    pub normalized_content: String,
    pub context_before: String,
    pub context_after: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PositionResponse {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub position_id: i64,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub anchor_id: i64,
    pub commit_sha: String,
    pub line_number: i32,
    pub confidence: i32,
    pub position_status: PositionStatus,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CommentReviewResponse {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub comment_id: i64,
    pub user_name: String,
    pub content: Option<String>,
    #[serde(
        default,
        serialize_with = "serialize_option_i64_as_string",
        deserialize_with = "deserialize_option_i64_from_string_or_number"
    )]
    #[schema(value_type = Option<String>)]
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CodeReviewViews> for CodeReviewResponse {
    fn from(value: CodeReviewViews) -> Self {
        Self {
            link: value.link,
            files: value.files.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FileReviewView> for FileReviewResponse {
    fn from(value: FileReviewView) -> Self {
        Self {
            file_path: value.file_path,
            threads: value.threads.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ThreadReviewView> for ThreadReviewResponse {
    fn from(value: ThreadReviewView) -> Self {
        Self {
            thread_id: value.thread_id,
            status: value.status.into(),
            created_at: value.created_at.to_string(),
            updated_at: value.updated_at.to_string(),
            anchor: value.anchor.into(),
            position: value.position.into(),
            comments: value.comments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<AnchorView> for AnchorResponse {
    fn from(value: AnchorView) -> Self {
        Self {
            anchor_id: value.anchor_id,
            file_path: value.file_path,
            diff_side: value.diff_side.into(),
            anchor_commit_sha: value.anchor_commit_sha,
            original_line_number: value.original_line_number,
            normalized_content: value.normalized_content,
            context_before: value.context_before,
            context_after: value.context_after,
        }
    }
}

impl From<PositionView> for PositionResponse {
    fn from(value: PositionView) -> Self {
        Self {
            position_id: value.position_id,
            anchor_id: value.anchor_id,
            commit_sha: value.commit_sha,
            line_number: value.line_number,
            confidence: value.confidence,
            position_status: value.position_status.into(),
        }
    }
}

impl From<CommentReviewView> for CommentReviewResponse {
    fn from(value: CommentReviewView) -> Self {
        Self {
            comment_id: value.comment_id,
            user_name: value.user_name,
            content: value.content,
            parent_id: value.parent_id,
            created_at: value.created_at.to_string(),
            updated_at: value.updated_at.to_string(),
        }
    }
}

impl From<mega_code_review_thread::Model> for ThreadStatusResponse {
    fn from(value: mega_code_review_thread::Model) -> Self {
        Self {
            thread_id: value.id,
            link: value.link,
            status: value.thread_status.into(),
        }
    }
}

impl From<DiffSideEnum> for DiffSide {
    fn from(value: DiffSideEnum) -> Self {
        match value {
            DiffSideEnum::Old => DiffSide::Deletions,
            DiffSideEnum::New => DiffSide::Additions,
        }
    }
}

impl From<DiffSide> for DiffSideEnum {
    fn from(value: DiffSide) -> Self {
        match value {
            DiffSide::Deletions => DiffSideEnum::Old,
            DiffSide::Additions => DiffSideEnum::New,
        }
    }
}

impl From<ThreadStatusEnum> for ThreadStatus {
    fn from(value: ThreadStatusEnum) -> Self {
        match value {
            ThreadStatusEnum::Open => ThreadStatus::Open,
            ThreadStatusEnum::Resolved => ThreadStatus::Resolved,
        }
    }
}

impl From<ThreadStatus> for ThreadStatusEnum {
    fn from(value: ThreadStatus) -> Self {
        match value {
            ThreadStatus::Open => ThreadStatusEnum::Open,
            ThreadStatus::Resolved => ThreadStatusEnum::Resolved,
        }
    }
}

impl From<PositionStatus> for PositionStatusEnum {
    fn from(value: PositionStatus) -> Self {
        match value {
            PositionStatus::Exact => PositionStatusEnum::Exact,
            PositionStatus::Shifted => PositionStatusEnum::Shifted,
            PositionStatus::PendingReanchor => PositionStatusEnum::PendingReanchor,
            PositionStatus::Ambiguous => PositionStatusEnum::Ambiguous,
            PositionStatus::NotFound => PositionStatusEnum::NotFound,
        }
    }
}

impl From<PositionStatusEnum> for PositionStatus {
    fn from(value: PositionStatusEnum) -> Self {
        match value {
            PositionStatusEnum::Exact => PositionStatus::Exact,
            PositionStatusEnum::Shifted => PositionStatus::Shifted,
            PositionStatusEnum::PendingReanchor => PositionStatus::PendingReanchor,
            PositionStatusEnum::Ambiguous => PositionStatus::Ambiguous,
            PositionStatusEnum::NotFound => PositionStatus::NotFound,
        }
    }
}
