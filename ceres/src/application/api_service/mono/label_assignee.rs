use std::collections::HashSet;

use callisto::sea_orm_active_enums::ConvTypeEnum;
use common::errors::MegaError;
use jupiter::model::common::LabelAssigneeParams;

use super::context::IssueApplicationService;
use crate::application::member_identity::display_label_for_actor;

impl IssueApplicationService {
    pub async fn update_item_labels(
        &self,
        username: &str,
        item_id: i64,
        item_type: &str,
        label_ids: Vec<i64>,
        link: &str,
    ) -> Result<(), MegaError> {
        let issue_storage = self.ctx.storage().issue_service.issue_store();

        let old_labels = issue_storage.find_item_exist_labels(item_id).await?;
        let old_ids: HashSet<i64> = old_labels.iter().map(|l| l.label_id).collect();
        let new_ids: HashSet<i64> = label_ids.iter().copied().collect();

        let to_add: Vec<i64> = new_ids.difference(&old_ids).copied().collect();
        let to_remove: Vec<i64> = old_ids.difference(&new_ids).copied().collect();

        let params = LabelAssigneeParams {
            item_id,
            item_type: item_type.to_string(),
        };

        issue_storage
            .modify_labels(to_add.clone(), to_remove.clone(), params)
            .await?;

        let display = display_label_for_actor(self.ctx.storage(), username).await;

        if !to_remove.is_empty() {
            self.ctx
                .storage()
                .issue_service
                .conversation_store()
                .add_conversation(
                    link,
                    username,
                    Some(format!("{display} removed {to_remove:?}")),
                    ConvTypeEnum::Label,
                )
                .await?;
        }

        if !to_add.is_empty() {
            self.ctx
                .storage()
                .issue_service
                .conversation_store()
                .add_conversation(
                    link,
                    username,
                    Some(format!("{display} added {to_add:?}")),
                    ConvTypeEnum::Label,
                )
                .await?;
        }

        Ok(())
    }

    pub async fn update_item_assignees(
        &self,
        username: &str,
        item_id: i64,
        item_type: &str,
        assignees: Vec<String>,
        link: &str,
    ) -> Result<(), MegaError> {
        let issue_storage = self.ctx.storage().issue_service.issue_store();

        let old_models = issue_storage.find_item_exist_assignees(item_id).await?;
        let old_ids: HashSet<String> = old_models
            .iter()
            .map(|m| m.campsite_user_id.clone())
            .collect();
        let new_ids: HashSet<String> = assignees.iter().cloned().collect();

        let to_add: Vec<String> = new_ids.difference(&old_ids).cloned().collect();
        let to_remove: Vec<String> = old_ids.difference(&new_ids).cloned().collect();

        let params = LabelAssigneeParams {
            item_id,
            item_type: item_type.to_string(),
        };

        issue_storage
            .modify_assignees(to_add.clone(), to_remove.clone(), params)
            .await?;

        let display = display_label_for_actor(self.ctx.storage(), username).await;

        if !to_remove.is_empty() {
            self.ctx
                .storage()
                .issue_service
                .conversation_store()
                .add_conversation(
                    link,
                    username,
                    Some(format!("{display} unassigned {to_remove:?}")),
                    ConvTypeEnum::Assignee,
                )
                .await?;
        }

        if !to_add.is_empty() {
            self.ctx
                .storage()
                .issue_service
                .conversation_store()
                .add_conversation(
                    link,
                    username,
                    Some(format!("{display} assigned {to_add:?}")),
                    ConvTypeEnum::Assignee,
                )
                .await?;
        }

        Ok(())
    }
}
