use std::ops::Deref;

use callisto::cla_sign_status;
use common::errors::MegaError;
use sea_orm::{
    ColumnTrait, DbErr, EntityTrait, QueryFilter, QuerySelect, Set, prelude::Expr,
    sea_query::OnConflict,
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone, Debug)]
pub struct ClaStorage {
    pub base: BaseStorage,
}

impl Deref for ClaStorage {
    type Target = BaseStorage;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ClaStorage {
    fn handle_record_not_inserted<T>(result: Result<T, DbErr>) -> Result<(), MegaError> {
        match result {
            Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_status(
        &self,
        username: &str,
    ) -> Result<Option<cla_sign_status::Model>, MegaError> {
        Ok(cla_sign_status::Entity::find_by_id(username.to_string())
            .one(self.get_connection())
            .await?)
    }

    pub async fn get_or_create_status(
        &self,
        username: &str,
    ) -> Result<cla_sign_status::Model, MegaError> {
        let now = chrono::Utc::now().naive_utc();
        let model = cla_sign_status::ActiveModel {
            campsite_user_id: Set(username.to_string()),
            cla_signed: Set(false),
            cla_signed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };

        Self::handle_record_not_inserted(
            cla_sign_status::Entity::insert(model)
                .on_conflict(
                    OnConflict::column(cla_sign_status::Column::CampsiteUserId)
                        .do_nothing()
                        .to_owned(),
                )
                .exec(self.get_connection())
                .await,
        )?;

        self.get_status(username)
            .await?
            .ok_or_else(|| MegaError::Other("Failed to get or create CLA status".to_string()))
    }

    pub async fn is_signed(&self, username: &str) -> Result<bool, MegaError> {
        Ok(self
            .get_status(username)
            .await?
            .map(|status| status.cla_signed)
            .unwrap_or(false))
    }

    pub async fn sign(&self, username: &str) -> Result<cla_sign_status::Model, MegaError> {
        let now = chrono::Utc::now().naive_utc();

        let active_model = cla_sign_status::ActiveModel {
            campsite_user_id: Set(username.to_string()),
            cla_signed: Set(true),
            cla_signed_at: Set(Some(now)),
            created_at: Set(now),
            updated_at: Set(now),
        };

        Self::handle_record_not_inserted(
            cla_sign_status::Entity::insert(active_model)
                .on_conflict(
                    OnConflict::column(cla_sign_status::Column::CampsiteUserId)
                        .do_nothing()
                        .to_owned(),
                )
                .exec(self.get_connection())
                .await,
        )?;

        cla_sign_status::Entity::update_many()
            .col_expr(cla_sign_status::Column::ClaSigned, Expr::value(true))
            .col_expr(cla_sign_status::Column::ClaSignedAt, Expr::value(now))
            .col_expr(cla_sign_status::Column::UpdatedAt, Expr::value(now))
            .filter(cla_sign_status::Column::CampsiteUserId.eq(username))
            .filter(cla_sign_status::Column::ClaSigned.eq(false))
            .exec(self.get_connection())
            .await?;

        let model = self
            .get_status(username)
            .await?
            .ok_or_else(|| MegaError::Other("Failed to sign CLA status".to_string()))?;

        if !model.cla_signed {
            return Err(MegaError::Other(format!(
                "CLA status for user `{username}` still unsigned after sign operation (cla_signed={}) ",
                model.cla_signed
            )));
        }

        Ok(model)
    }

    pub async fn unsigned_users(&self, usernames: &[String]) -> Result<Vec<String>, MegaError> {
        if usernames.is_empty() {
            return Ok(Vec::new());
        }

        let signed_users: Vec<String> = cla_sign_status::Entity::find()
            .select_only()
            .column(cla_sign_status::Column::CampsiteUserId)
            .filter(cla_sign_status::Column::CampsiteUserId.is_in(usernames.iter().cloned()))
            .filter(cla_sign_status::Column::ClaSigned.eq(true))
            .into_tuple::<String>()
            .all(self.get_connection())
            .await?;

        let unsigned = usernames
            .iter()
            .filter(|username| !signed_users.contains(*username))
            .cloned()
            .collect();

        Ok(unsigned)
    }
}
