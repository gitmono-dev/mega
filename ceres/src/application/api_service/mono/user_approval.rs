//! User approval status operations for [`UserApplicationService`].

use callisto::user_approval_status;
use common::errors::MegaError;
use jupiter::storage::user_approval_storage::{
    APPROVAL_STATUS_APPROVED, APPROVAL_STATUS_REJECTED, UserApprovalProfile,
};

use super::context::UserApplicationService;
use crate::model::user::UserApprovalStatusRes;

impl UserApplicationService {
    pub async fn get_or_init_user_approval_status(
        &self,
        username: &str,
        campsite_user_id: &str,
        display_name: &str,
        email: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .get_or_create(
                username,
                UserApprovalProfile {
                    campsite_user_id: campsite_user_id.to_string(),
                    display_name: display_name.to_string(),
                    email: email.to_string(),
                },
            )
            .await
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

    pub async fn approve_user(
        &self,
        username: &str,
        reviewed_by: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .set_status(username, APPROVAL_STATUS_APPROVED, reviewed_by)
            .await
    }

    pub async fn reject_user(
        &self,
        username: &str,
        reviewed_by: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        self.ctx
            .storage()
            .user_approval_storage()
            .set_status(username, APPROVAL_STATUS_REJECTED, reviewed_by)
            .await
    }
}

impl From<user_approval_status::Model> for UserApprovalStatusRes {
    fn from(value: user_approval_status::Model) -> Self {
        Self {
            username: value.username,
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
