use common::errors::MegaError;

use super::context::CodeReviewApplicationService;
use crate::model::code_review::{
    CodeReviewResponse, CommentReplyRequest, CommentReviewResponse, InitializeCommentRequest,
    ThreadReviewResponse, ThreadStatusResponse, UpdateCommentRequest,
};

impl CodeReviewApplicationService {
    pub async fn get_code_review_comments(
        &self,
        link: &str,
    ) -> Result<CodeReviewResponse, MegaError> {
        let comments = self
            .ctx
            .storage()
            .code_review_service
            .get_all_comments_by_link(link)
            .await?;
        Ok(comments.into())
    }

    pub async fn create_code_review_comment(
        &self,
        link: &str,
        username: String,
        payload: InitializeCommentRequest,
    ) -> Result<ThreadReviewResponse, MegaError> {
        let thread = self
            .ctx
            .storage()
            .code_review_service
            .create_inline_comment(
                link,
                &payload.file_path,
                payload.diff_side.into(),
                &payload.anchor_commit_sha,
                payload.original_line_number,
                &payload.normalized_content,
                &payload.context_before,
                &payload.context_after,
                username,
                payload.content,
            )
            .await?;
        Ok(thread.into())
    }

    pub async fn reply_code_review_comment(
        &self,
        thread_id: i64,
        username: String,
        payload: CommentReplyRequest,
    ) -> Result<CommentReviewResponse, MegaError> {
        let comment = self
            .ctx
            .storage()
            .code_review_service
            .reply_to_comment(
                thread_id,
                payload.parent_comment_id,
                username,
                payload.content,
            )
            .await?;
        Ok(comment.into())
    }

    pub async fn update_code_review_comment(
        &self,
        comment_id: i64,
        username: &str,
        payload: UpdateCommentRequest,
    ) -> Result<CommentReviewResponse, MegaError> {
        let comment = self
            .ctx
            .storage()
            .code_review_comment_storage()
            .find_comment_by_id(comment_id)
            .await?
            .ok_or_else(|| MegaError::NotFound("Comment not found".to_string()))?;

        if comment.campsite_user_id != username {
            return Err(MegaError::Other(
                "Cannot update others' comments".to_string(),
            ));
        }

        let updated = self
            .ctx
            .storage()
            .code_review_service
            .update_comment(comment_id, payload.content)
            .await?;
        Ok(updated.into())
    }

    pub async fn resolve_code_review_thread(
        &self,
        thread_id: i64,
    ) -> Result<ThreadStatusResponse, MegaError> {
        let thread = self
            .ctx
            .storage()
            .code_review_service
            .resolve_thread(thread_id)
            .await?;
        Ok(thread.into())
    }

    pub async fn reopen_code_review_thread(
        &self,
        thread_id: i64,
    ) -> Result<ThreadStatusResponse, MegaError> {
        let thread = self
            .ctx
            .storage()
            .code_review_service
            .reopen_thread(thread_id)
            .await?;
        Ok(thread.into())
    }

    pub async fn delete_code_review_thread(&self, thread_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .code_review_service
            .delete_thread(thread_id)
            .await?;
        Ok(())
    }

    pub async fn delete_code_review_comment(
        &self,
        comment_id: i64,
        username: &str,
    ) -> Result<(), MegaError> {
        let comment = self
            .ctx
            .storage()
            .code_review_comment_storage()
            .find_comment_by_id(comment_id)
            .await?
            .ok_or_else(|| MegaError::NotFound("Comment not found".to_string()))?;

        if comment.campsite_user_id != username {
            return Err(MegaError::Other(
                "Cannot update others' comments".to_string(),
            ));
        }

        self.ctx
            .storage()
            .code_review_service
            .delete_comment(comment_id)
            .await?;
        Ok(())
    }
}
