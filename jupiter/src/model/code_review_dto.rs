// This module defines the database entities and DTOs for a Code Review system,
// using SeaORM for Rust. The design follows a three-layered structure:
//
// 1. Thread (`mega_code_review_thread`) – represents the discussion identity,
//    manages overall thread status (open/resolved), and serves as the aggregate root
//    for comments. It does not directly track code positions.
//
// 2. Anchor (`mega_code_review_anchor`) – immutable historical anchor that records
//    the original code segment being commented on, including normalized content,
//    context lines, line number, file path, diff side, and the commit SHA at creation.
//    Anchors are never updated and serve as the basis for re-anchoring positions.
//
// 3. Position (`mega_code_review_position`) – derived, recomputable entity that
//    maps an Anchor to its current location in a given commit, including file path,
//    line number, diff side, confidence score, and status (ok/moved/outdated).
//    Positions are recalculated on code changes to automatically handle comment drift.
//
// 4. Comment (`mega_code_review_comment`) – stores individual comments linked
//    to a Thread, with optional parent-child relationships for threaded discussions.
//
// DTOs (CodeReviewViews, FileReviewView, ThreadReviewView, AnchorView, PositionView,
// CommentReviewView) are structured to reflect these layers for front-end rendering,
// keeping Anchor immutable, Position dynamic, and Thread as the stable discussion entity.
use callisto::{
    mega_code_review_anchor, mega_code_review_comment, mega_code_review_position,
    mega_code_review_thread,
    sea_orm_active_enums::{DiffSideEnum, PositionStatusEnum, ThreadStatusEnum},
};

pub struct CodeReviewViews {
    pub link: String,
    pub files: Vec<FileReviewView>,
}

pub struct FileReviewView {
    pub file_path: String,
    pub threads: Vec<ThreadReviewView>,
}

// Thread DTO
pub struct ThreadReviewView {
    pub thread_id: i64,
    pub status: ThreadStatusEnum,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub anchor: AnchorView,
    pub position: PositionView,
    pub comments: Vec<CommentReviewView>,
}

// Anchor DTO
pub struct AnchorView {
    pub anchor_id: i64,
    pub file_path: String,
    pub diff_side: DiffSideEnum,
    pub original_line_number: i32,
    pub normalized_content: String,
    pub context_before: String,
    pub context_after: String,
    pub anchor_commit_sha: String,
}

// Position DTO
pub struct PositionView {
    pub position_id: i64,
    pub anchor_id: i64,
    pub file_path: String,
    pub diff_side: DiffSideEnum,
    pub line_number: i32,
    pub confidence: i32,
    pub position_status: PositionStatusEnum,
    pub commit_sha: String,
}

// Comment DTO
pub struct CommentReviewView {
    pub comment_id: i64,
    pub user_name: String,
    pub content: Option<String>,
    pub parent_id: Option<i64>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl From<mega_code_review_comment::Model> for CommentReviewView {
    fn from(value: mega_code_review_comment::Model) -> Self {
        Self {
            comment_id: value.id,
            user_name: value.campsite_user_id,
            content: value.content,
            parent_id: value.parent_id,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

// Thread -> ThreadReviewView
impl ThreadReviewView {
    pub fn from_models(
        thread: mega_code_review_thread::Model,
        anchor: mega_code_review_anchor::Model,
        position: mega_code_review_position::Model,
        comments: Vec<mega_code_review_comment::Model>,
    ) -> Self {
        Self {
            thread_id: thread.id,
            status: thread.thread_status,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            anchor: AnchorView {
                anchor_id: anchor.id,
                file_path: anchor.file_path,
                diff_side: anchor.diff_side,
                original_line_number: anchor.original_line_number,
                normalized_content: anchor.normalized_content,
                context_before: anchor.context_before,
                context_after: anchor.context_after,
                anchor_commit_sha: anchor.anchor_commit_sha,
            },
            position: PositionView {
                position_id: position.id,
                anchor_id: anchor.id,
                file_path: position.file_path,
                diff_side: position.diff_side,
                line_number: position.line_number,
                confidence: position.confidence,
                position_status: position.position_status,
                commit_sha: position.commit_sha,
            },
            comments: comments.into_iter().map(CommentReviewView::from).collect(),
        }
    }
}
