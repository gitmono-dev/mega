//! CL lifecycle orchestration (detail, status transitions, comments, merge box).

use std::collections::HashSet;

use callisto::sea_orm_active_enums::{ConvTypeEnum, MergeStatusEnum};
use common::errors::MegaError;
use jupiter::model::cl_dto::CLDetails;

use crate::{
    application::{
        api_service::mono::ClApplicationService,
        member_identity::display_label_for_actor,
        webhook::{WebhookEvent, dispatch_cl_webhook},
    },
    model::change_list::{CLDetailRes, Condition, MergeBoxRes, UpdateClStatusPayload},
};

impl ClApplicationService {
    pub async fn get_cl_details(
        &self,
        link: &str,
        username: String,
    ) -> Result<CLDetailRes, MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let conversation_storage = self.storage().cl_service.conversation_store();

        let (cl, labels) = cl_storage
            .get_cl_labels(link)
            .await?
            .ok_or_else(|| MegaError::NotFound("CL not found".to_string()))?;

        let conversations = conversation_storage
            .get_comments_with_reactions(link)
            .await?;

        let (_, assignees) = cl_storage
            .get_cl_assignees(link)
            .await?
            .unwrap_or((cl.clone(), vec![]));

        let mut detail: CLDetailRes = CLDetails {
            cl,
            labels,
            conversations,
            assignees,
            username,
        }
        .into();

        let mut names: Vec<String> = detail
            .conversations
            .iter()
            .map(|c| c.username.clone())
            .collect();
        names.push(detail.author.clone());
        let bot_names = self
            .storage()
            .bots_storage()
            .bot_names_among(&names)
            .await?;
        detail.author_is_bot = bot_names.contains(&detail.author) || detail.author == "system";
        crate::model::conversation::ConversationItem::apply_bot_flags(
            &mut detail.conversations,
            &bot_names,
        );

        Ok(detail)
    }

    pub async fn reopen_cl(&self, link: &str, actor: &str) -> Result<(), MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let model = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        if model.status != MergeStatusEnum::Closed {
            return Ok(());
        }

        let link = model.link.clone();
        cl_storage.reopen_cl(model.clone()).await?;
        let display = display_label_for_actor(self.storage(), actor).await;
        self.storage()
            .cl_service
            .conversation_store()
            .add_conversation(
                &link,
                actor,
                Some(format!("{display} reopen this")),
                ConvTypeEnum::Reopen,
            )
            .await?;

        if let Some(updated_model) = cl_storage.get_cl(&link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClReopened, &updated_model);
        }
        Ok(())
    }

    pub async fn close_cl(&self, link: &str, actor: &str) -> Result<(), MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let model = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        if !matches!(model.status, MergeStatusEnum::Open | MergeStatusEnum::Draft) {
            return Ok(());
        }

        let link = model.link.clone();
        cl_storage.close_cl(model.clone()).await?;
        let display = display_label_for_actor(self.storage(), actor).await;
        self.storage()
            .cl_service
            .conversation_store()
            .add_conversation(
                &link,
                actor,
                Some(format!("{display} closed this")),
                ConvTypeEnum::Closed,
            )
            .await?;

        if let Some(updated_model) = cl_storage.get_cl(&link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClClosed, &updated_model);
        }
        Ok(())
    }

    pub async fn merge_open_cl(&self, actor: &str, link: &str) -> Result<(), MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let model = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        if model.status == MergeStatusEnum::Draft {
            return Err(MegaError::bad_request("CL is not ready for review"));
        }

        if model.status == MergeStatusEnum::Open {
            self.merge_cl(actor, model.clone()).await?;
            if let Some(updated_model) = cl_storage.get_cl(link).await? {
                dispatch_cl_webhook(self.storage(), WebhookEvent::ClMerged, &updated_model);
            }
        }
        Ok(())
    }

    pub async fn merge_open_cl_no_auth(&self, link: &str) -> Result<(), MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let model = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        if model.status != MergeStatusEnum::Open {
            return Err(MegaError::bad_request(format!(
                "CL is not in Open status, current status: {:?}",
                model.status
            )));
        }

        // Attribute the merge to the CL author (e.g. mega-init bot), not a
        // synthetic "system" user — so the timeline can show a bot badge/avatar.
        let merge_actor = model.campsite_user_id.clone();
        self.merge_cl(&merge_actor, model.clone()).await?;
        if let Some(updated_model) = cl_storage.get_cl(link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClMerged, &updated_model);
        }
        Ok(())
    }

    pub async fn get_merge_box(&self, link: &str) -> Result<MergeBoxRes, MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let cl = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        let res = match cl.status {
            MergeStatusEnum::Open => {
                let check_res: Vec<Condition> = cl_storage
                    .get_check_result(link)
                    .await?
                    .into_iter()
                    .map(|m| m.into())
                    .collect();
                MergeBoxRes::from_condition(check_res)
            }
            MergeStatusEnum::Draft | MergeStatusEnum::Merged | MergeStatusEnum::Closed => {
                MergeBoxRes {
                    merge_requirements: None,
                }
            }
        };
        Ok(res)
    }

    pub async fn save_cl_comment(
        &self,
        link: &str,
        actor: &str,
        content: &str,
    ) -> Result<(), MegaError> {
        let conv_type = if self
            .storage()
            .reviewer_storage()
            .is_reviewer(link, actor)
            .await?
        {
            ConvTypeEnum::Review
        } else {
            ConvTypeEnum::Comment
        };

        self.storage()
            .cl_service
            .conversation_store()
            .add_conversation(link, actor, Some(content.to_string()), conv_type)
            .await?;

        if let Err(e) = enqueue_cl_comment_notifications(self, actor, link, content).await {
            tracing::warn!("failed to enqueue cl comment notifications: {e}");
        }

        if let Some(cl_model) = self.storage().cl_service.cl_store().get_cl(link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClCommentCreated, &cl_model);
        }
        Ok(())
    }

    pub async fn edit_cl_title(&self, link: &str, content: &str) -> Result<(), MegaError> {
        self.storage()
            .cl_service
            .cl_store()
            .edit_title(link, content)
            .await?;
        if let Some(cl_model) = self.storage().cl_service.cl_store().get_cl(link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClUpdated, &cl_model);
        }
        Ok(())
    }

    pub async fn update_cl_status(
        &self,
        link: &str,
        actor: &str,
        payload: &UpdateClStatusPayload,
    ) -> Result<(), MegaError> {
        let cl_storage = self.storage().cl_service.cl_store();
        let model = cl_storage
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {link}")))?;

        let new_status = match payload.status.to_lowercase().as_str() {
            "draft" => MergeStatusEnum::Draft,
            "open" => MergeStatusEnum::Open,
            _ => {
                return Err(MegaError::bad_request(
                    "Invalid status. Only 'draft' and 'open' are supported",
                ));
            }
        };

        match (&model.status, &new_status) {
            (MergeStatusEnum::Draft, MergeStatusEnum::Open) => {
                cl_storage
                    .update_cl_status(model.clone(), new_status.clone())
                    .await?;
                let display = display_label_for_actor(self.storage(), actor).await;
                self.storage()
                    .cl_service
                    .conversation_store()
                    .add_conversation(
                        link,
                        actor,
                        Some(format!("{display} marked this as ready for review")),
                        ConvTypeEnum::Review,
                    )
                    .await?;
                if let Some(updated_model) = cl_storage.get_cl(link).await? {
                    dispatch_cl_webhook(self.storage(), WebhookEvent::ClCreated, &updated_model);
                }
            }
            (MergeStatusEnum::Open, MergeStatusEnum::Draft) => {
                cl_storage
                    .update_cl_status(model.clone(), new_status.clone())
                    .await?;
                let display = display_label_for_actor(self.storage(), actor).await;
                self.storage()
                    .cl_service
                    .conversation_store()
                    .add_conversation(
                        link,
                        actor,
                        Some(format!("{display} marked this as draft")),
                        ConvTypeEnum::Draft,
                    )
                    .await?;
                if let Some(updated_model) = cl_storage.get_cl(link).await? {
                    dispatch_cl_webhook(self.storage(), WebhookEvent::ClUpdated, &updated_model);
                }
            }
            _ => {
                return Err(MegaError::bad_request(
                    "Invalid status transition. Only Draft ↔ Open is allowed",
                ));
            }
        }
        Ok(())
    }

    pub async fn update_branch_with_webhook(
        &self,
        actor: &str,
        link: &str,
    ) -> Result<String, MegaError> {
        let new_head = self.update_branch(actor, link).await?;
        if let Some(cl_model) = self.storage().cl_service.cl_store().get_cl(link).await? {
            dispatch_cl_webhook(self.storage(), WebhookEvent::ClUpdated, &cl_model);
        }
        Ok(new_head)
    }
}

const EVENT_CL_COMMENT_CREATED: &str = "cl.comment.created";

async fn enqueue_cl_comment_notifications(
    service: &ClApplicationService,
    actor: &str,
    cl_link: &str,
    comment_text: &str,
) -> Result<(), MegaError> {
    let notif_stg = service.storage().notification_storage();
    ensure_cl_comment_event_type(&notif_stg).await?;

    let cl_stg = service.storage().cl_service.cl_store();
    let cl = cl_stg
        .get_cl(cl_link)
        .await?
        .ok_or_else(|| MegaError::NotFound(format!("CL {cl_link} not found")))?;

    let reviewers = service
        .storage()
        .reviewer_storage()
        .list_reviewers(cl_link)
        .await?;

    let mut recipients: HashSet<String> = HashSet::new();
    recipients.insert(cl.campsite_user_id);
    for r in reviewers {
        recipients.insert(r.campsite_user_id);
    }
    recipients.remove(actor);

    for username in recipients {
        if !notif_stg
            .should_send(&username, EVENT_CL_COMMENT_CREATED)
            .await?
        {
            continue;
        }

        let settings = match notif_stg.get_user_settings(&username).await? {
            Some(s) => s,
            None => continue,
        };

        let subject = format!("New comment on CL {cl_link}");
        let body_text = format!("{actor} commented on {cl_link}: {comment_text}");
        let body_html = format!(
            "<p><b>{actor}</b> commented on <b>{cl_link}</b>:</p><p>{}</p>",
            escape_html(comment_text)
        );

        notif_stg
            .enqueue_email_job(
                &username,
                &settings.email,
                EVENT_CL_COMMENT_CREATED,
                &subject,
                &body_html,
                Some(&body_text),
            )
            .await?;
    }

    Ok(())
}

async fn ensure_cl_comment_event_type(
    notif_stg: &jupiter::storage::NotificationStorage,
) -> Result<(), MegaError> {
    use callisto::notification_event_types;
    use jupiter::sea_orm::{ActiveModelTrait, Set};

    if notif_stg
        .get_event_type(EVENT_CL_COMMENT_CREATED)
        .await?
        .is_some()
    {
        return Ok(());
    }

    let now = chrono::Utc::now().naive_utc();
    notification_event_types::ActiveModel {
        code: Set(EVENT_CL_COMMENT_CREATED.to_owned()),
        category: Set("cl".to_owned()),
        description: Set("New comment on a Change List".to_owned()),
        system_required: Set(false),
        default_enabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(notif_stg.db())
    .await?;

    Ok(())
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
