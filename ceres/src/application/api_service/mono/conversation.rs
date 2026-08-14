use common::errors::MegaError;

use super::context::ConversationApplicationService;
use crate::{
    application::member_identity::display_label_for_actor,
    model::{
        change_list::MergeStatus,
        conversation::{ConvType, ReferenceType},
    },
};

impl ConversationApplicationService {
    pub async fn add_conversation(
        &self,
        link: &str,
        username: &str,
        comment: Option<String>,
        conv_type: ConvType,
    ) -> Result<i64, MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .add_conversation(link, username, comment, conv_type.into())
            .await
    }

    /// System timeline event: persist actor as campsite public id, comment uses display label.
    pub async fn add_system_event(
        &self,
        link: &str,
        actor: &str,
        phrase: &str,
        conv_type: ConvType,
    ) -> Result<i64, MegaError> {
        let display = display_label_for_actor(self.ctx.storage(), actor).await;
        self.add_conversation(link, actor, Some(format!("{display} {phrase}")), conv_type)
            .await
    }

    pub async fn add_issue_mention_reference(
        &self,
        source_link: &str,
        ref_link: &str,
        username: &str,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .issue_store()
            .add_reference(source_link, ref_link, ReferenceType::Mention.into())
            .await?;
        self.add_system_event(ref_link, username, "mentioned this on", ConvType::Mention)
            .await?;
        Ok(())
    }

    pub async fn cl_merge_status(&self, link: &str) -> Result<MergeStatus, MegaError> {
        let model = self
            .ctx
            .storage()
            .cl_service
            .cl_store()
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL {link} not found")))?;
        Ok(model.status.into())
    }

    pub async fn add_comment_reaction(
        &self,
        content: Option<String>,
        comment_id: i64,
        comment_type: &str,
        username: &str,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .add_reactions(content, comment_id, comment_type, username)
            .await?;
        Ok(())
    }

    pub async fn delete_comment_reaction(
        &self,
        reaction_id: &str,
        username: &str,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .delete_reaction(reaction_id, username)
            .await
    }

    pub async fn remove_conversation(&self, comment_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .remove_conversation(comment_id)
            .await
    }

    pub async fn update_comment(
        &self,
        comment_id: i64,
        content: Option<String>,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .update_comment(comment_id, content)
            .await
    }

    pub async fn change_review_state(
        &self,
        link: &str,
        conversation_id: &i64,
        resolved: bool,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .conversation_store()
            .change_review_state(link, conversation_id, resolved)
            .await
    }
}
