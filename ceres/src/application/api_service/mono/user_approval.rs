//! User approval status operations for [`UserApplicationService`].

use callisto::user_approval_status;
use common::errors::MegaError;
use jupiter::storage::{
    campsite_member_identity_storage::MemberIdentityProfile,
    user_approval_storage::{
        APPROVAL_STATUS_APPROVED, APPROVAL_STATUS_REJECTED, UserApprovalProfile,
    },
};

use super::context::UserApplicationService;
use crate::{
    application::member_identity::{display_labels_for_actors, upsert_local_identity},
    model::user::UserApprovalStatusRes,
};

impl UserApplicationService {
    pub async fn get_or_init_user_approval_status(
        &self,
        campsite_user_id: &str,
        display_name: &str,
        email: &str,
        github_login: Option<&str>,
    ) -> Result<user_approval_status::Model, MegaError> {
        let model = self
            .ctx
            .storage()
            .user_approval_storage()
            .get_or_create(UserApprovalProfile {
                campsite_user_id: campsite_user_id.to_string(),
                display_name: display_name.to_string(),
                email: email.to_string(),
            })
            .await?;

        let login = github_login
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let _ = upsert_local_identity(
            self.ctx.storage(),
            MemberIdentityProfile {
                campsite_user_id: campsite_user_id.to_string(),
                username: login.clone().unwrap_or_default(),
                github_login: login,
                display_name: display_name.to_string(),
                email: email.to_string(),
            },
        )
        .await;

        Ok(model)
    }

    pub async fn list_user_approvals(
        &self,
        status: Option<&str>,
        limit: u64,
    ) -> Result<Vec<user_approval_status::Model>, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .list_by_status(status, limit)
            .await
    }

    pub async fn list_user_approval_responses(
        &self,
        status: Option<&str>,
        limit: u64,
    ) -> Result<Vec<UserApprovalStatusRes>, MegaError> {
        let models = self.list_user_approvals(status, limit).await?;
        let reviewer_ids: Vec<String> = models
            .iter()
            .filter_map(|m| m.reviewed_by.clone())
            .collect();
        let labels = display_labels_for_actors(self.ctx.storage(), &reviewer_ids).await;
        Ok(models
            .into_iter()
            .map(|model| Self::model_to_res(model, &labels))
            .collect())
    }

    pub async fn approve_user(
        &self,
        campsite_user_id: &str,
        reviewed_by: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .set_status(campsite_user_id, APPROVAL_STATUS_APPROVED, reviewed_by)
            .await
    }

    pub async fn reject_user(
        &self,
        campsite_user_id: &str,
        reviewed_by: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .set_status(campsite_user_id, APPROVAL_STATUS_REJECTED, reviewed_by)
            .await
    }

    /// Upsert the reviewer's local identity, then approve.
    pub async fn approve_user_response(
        &self,
        campsite_user_id: &str,
        reviewed_by: &str,
        reviewer: MemberIdentityProfile,
    ) -> Result<UserApprovalStatusRes, MegaError> {
        let _ = upsert_local_identity(self.ctx.storage(), reviewer).await;
        let model = self.approve_user(campsite_user_id, reviewed_by).await?;
        Ok(self.to_approval_status_res(model).await)
    }

    /// Upsert the reviewer's local identity, then reject.
    pub async fn reject_user_response(
        &self,
        campsite_user_id: &str,
        reviewed_by: &str,
        reviewer: MemberIdentityProfile,
    ) -> Result<UserApprovalStatusRes, MegaError> {
        let _ = upsert_local_identity(self.ctx.storage(), reviewer).await;
        let model = self.reject_user(campsite_user_id, reviewed_by).await?;
        Ok(self.to_approval_status_res(model).await)
    }

    pub async fn to_approval_status_res(
        &self,
        model: user_approval_status::Model,
    ) -> UserApprovalStatusRes {
        let reviewer_ids: Vec<String> = model.reviewed_by.iter().cloned().collect();
        let labels = display_labels_for_actors(self.ctx.storage(), &reviewer_ids).await;
        Self::model_to_res(model, &labels)
    }

    fn model_to_res(
        model: user_approval_status::Model,
        labels: &std::collections::HashMap<String, String>,
    ) -> UserApprovalStatusRes {
        let mut res = UserApprovalStatusRes::from(model);
        if let Some(label) = res
            .reviewed_by
            .as_deref()
            .and_then(|reviewed_by| labels.get(reviewed_by))
        {
            res.reviewed_by = Some(label.clone());
        }
        res
    }
}

impl From<user_approval_status::Model> for UserApprovalStatusRes {
    fn from(value: user_approval_status::Model) -> Self {
        Self {
            // API compat: `username` mirrors campsite_user_id after username column drop.
            username: value.campsite_user_id.clone(),
            campsite_user_id: value.campsite_user_id,
            display_name: value.display_name,
            email: value.email,
            status: value.status,
            reviewed_by: value.reviewed_by,
            reviewed_at: value.reviewed_at.map(|dt| dt.and_utc().timestamp()),
            registered_at: value.created_at.and_utc().timestamp(),
        }
    }
}
