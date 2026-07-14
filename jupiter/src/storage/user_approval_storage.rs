use std::ops::Deref;

use callisto::user_approval_status;
use common::errors::MegaError;
use sea_orm::{
    ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, prelude::Expr,
    sea_query::OnConflict,
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

pub const APPROVAL_STATUS_PENDING: &str = "pending";
pub const APPROVAL_STATUS_APPROVED: &str = "approved";
pub const APPROVAL_STATUS_REJECTED: &str = "rejected";

#[derive(Clone, Debug)]
pub struct UserApprovalProfile {
    pub campsite_user_id: String,
    pub display_name: String,
    pub email: String,
}

#[derive(Clone, Debug)]
pub struct UserApprovalStorage {
    pub base: BaseStorage,
}

impl Deref for UserApprovalStorage {
    type Target = BaseStorage;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl UserApprovalStorage {
    fn handle_record_not_inserted<T>(result: Result<T, DbErr>) -> Result<(), MegaError> {
        match result {
            Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get(
        &self,
        username: &str,
    ) -> Result<Option<user_approval_status::Model>, MegaError> {
        Ok(
            user_approval_status::Entity::find_by_id(username.to_string())
                .one(self.get_connection())
                .await?,
        )
    }

    /// Create a pending row if missing; refresh profile fields if already present.
    pub async fn get_or_create(
        &self,
        username: &str,
        profile: UserApprovalProfile,
    ) -> Result<user_approval_status::Model, MegaError> {
        let now = chrono::Utc::now().naive_utc();
        let model = user_approval_status::ActiveModel {
            username: Set(username.to_string()),
            campsite_user_id: Set(profile.campsite_user_id.clone()),
            display_name: Set(profile.display_name.clone()),
            email: Set(profile.email.clone()),
            status: Set(APPROVAL_STATUS_PENDING.to_string()),
            reviewed_by: Set(None),
            reviewed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };

        Self::handle_record_not_inserted(
            user_approval_status::Entity::insert(model)
                .on_conflict(
                    OnConflict::column(user_approval_status::Column::Username)
                        .do_nothing()
                        .to_owned(),
                )
                .exec(self.get_connection())
                .await,
        )?;

        // Keep profile fields fresh for list display
        user_approval_status::Entity::update_many()
            .col_expr(
                user_approval_status::Column::CampsiteUserId,
                Expr::value(profile.campsite_user_id),
            )
            .col_expr(
                user_approval_status::Column::DisplayName,
                Expr::value(profile.display_name),
            )
            .col_expr(
                user_approval_status::Column::Email,
                Expr::value(profile.email),
            )
            .col_expr(user_approval_status::Column::UpdatedAt, Expr::value(now))
            .filter(user_approval_status::Column::Username.eq(username))
            .exec(self.get_connection())
            .await?;

        self.get(username)
            .await?
            .ok_or_else(|| MegaError::Other("Failed to get or create user approval status".into()))
    }

    pub async fn list_by_status(
        &self,
        status: Option<&str>,
        limit: u64,
    ) -> Result<Vec<user_approval_status::Model>, MegaError> {
        let mut query = user_approval_status::Entity::find()
            .order_by_desc(user_approval_status::Column::CreatedAt);

        if let Some(status) = status
            && status != "all"
        {
            query = query.filter(user_approval_status::Column::Status.eq(status));
        }

        Ok(query.limit(limit).all(self.get_connection()).await?)
    }

    pub async fn set_status(
        &self,
        username: &str,
        status: &str,
        reviewed_by: &str,
    ) -> Result<user_approval_status::Model, MegaError> {
        if !matches!(
            status,
            APPROVAL_STATUS_PENDING | APPROVAL_STATUS_APPROVED | APPROVAL_STATUS_REJECTED
        ) {
            return Err(MegaError::Other(format!(
                "Invalid approval status: {status}"
            )));
        }

        let now = chrono::Utc::now().naive_utc();

        // Ensure row exists so approve/reject of unknown usernames still works for listed users
        let existing = self.get(username).await?;
        if existing.is_none() {
            return Err(MegaError::Other(format!(
                "User approval record not found for `{username}`"
            )));
        }

        user_approval_status::Entity::update_many()
            .col_expr(user_approval_status::Column::Status, Expr::value(status))
            .col_expr(
                user_approval_status::Column::ReviewedBy,
                Expr::value(reviewed_by.to_string()),
            )
            .col_expr(user_approval_status::Column::ReviewedAt, Expr::value(now))
            .col_expr(user_approval_status::Column::UpdatedAt, Expr::value(now))
            .filter(user_approval_status::Column::Username.eq(username))
            .exec(self.get_connection())
            .await?;

        self.get(username)
            .await?
            .ok_or_else(|| MegaError::Other("Failed to update user approval status".into()))
    }
}
