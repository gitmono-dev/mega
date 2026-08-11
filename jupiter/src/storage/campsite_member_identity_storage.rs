use std::ops::Deref;

use callisto::campsite_member_identity;
use common::errors::MegaError;
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, Set, sea_query::OnConflict};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone, Debug, Default)]
pub struct MemberIdentityProfile {
    pub campsite_user_id: String,
    pub username: String,
    pub github_login: Option<String>,
    pub display_name: String,
    pub email: String,
}

#[derive(Clone, Debug)]
pub struct CampsiteMemberIdentityStorage {
    pub base: BaseStorage,
}

impl Deref for CampsiteMemberIdentityStorage {
    type Target = BaseStorage;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl CampsiteMemberIdentityStorage {
    pub async fn get(
        &self,
        campsite_user_id: &str,
    ) -> Result<Option<campsite_member_identity::Model>, MegaError> {
        Ok(
            campsite_member_identity::Entity::find_by_id(campsite_user_id.to_string())
                .one(self.get_connection())
                .await?,
        )
    }

    pub async fn get_by_ids(
        &self,
        campsite_user_ids: &[String],
    ) -> Result<Vec<campsite_member_identity::Model>, MegaError> {
        if campsite_user_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(campsite_member_identity::Entity::find()
            .filter(
                campsite_member_identity::Column::CampsiteUserId.is_in(campsite_user_ids.to_vec()),
            )
            .all(self.get_connection())
            .await?)
    }

    pub async fn list_all(&self) -> Result<Vec<campsite_member_identity::Model>, MegaError> {
        Ok(campsite_member_identity::Entity::find()
            .all(self.get_connection())
            .await?)
    }

    /// Find rows whose campsite_user_id, username, or github_login matches `actor`.
    pub async fn find_by_actor(
        &self,
        actor: &str,
    ) -> Result<Option<campsite_member_identity::Model>, MegaError> {
        let actor = actor.trim();
        if actor.is_empty() {
            return Ok(None);
        }

        if let Some(row) = self.get(actor).await? {
            return Ok(Some(row));
        }

        Ok(campsite_member_identity::Entity::find()
            .filter(
                Condition::any()
                    .add(campsite_member_identity::Column::Username.eq(actor))
                    .add(campsite_member_identity::Column::GithubLogin.eq(actor)),
            )
            .one(self.get_connection())
            .await?)
    }

    /// Insert or merge identity fields. Non-empty incoming values overwrite;
    /// empty incoming strings do not wipe existing non-empty values.
    pub async fn upsert(&self, profile: MemberIdentityProfile) -> Result<(), MegaError> {
        let id = profile.campsite_user_id.trim();
        if id.is_empty() {
            return Ok(());
        }

        let now = chrono::Utc::now().naive_utc();
        let existing = self.get(id).await?;

        let username = non_empty_or(
            profile.username.trim(),
            existing.as_ref().map(|r| r.username.as_str()).unwrap_or(""),
        );
        let display_name = non_empty_or(
            profile.display_name.trim(),
            existing
                .as_ref()
                .map(|r| r.display_name.as_str())
                .unwrap_or(""),
        );
        let email = non_empty_or(
            profile.email.trim(),
            existing.as_ref().map(|r| r.email.as_str()).unwrap_or(""),
        );
        let github_login = profile
            .github_login
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| existing.and_then(|r| r.github_login));

        let model = campsite_member_identity::ActiveModel {
            campsite_user_id: Set(id.to_string()),
            username: Set(username),
            github_login: Set(github_login),
            display_name: Set(display_name),
            email: Set(email),
            updated_at: Set(now),
        };

        campsite_member_identity::Entity::insert(model)
            .on_conflict(
                OnConflict::column(campsite_member_identity::Column::CampsiteUserId)
                    .update_columns([
                        campsite_member_identity::Column::Username,
                        campsite_member_identity::Column::GithubLogin,
                        campsite_member_identity::Column::DisplayName,
                        campsite_member_identity::Column::Email,
                        campsite_member_identity::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(self.get_connection())
            .await?;

        Ok(())
    }

    pub async fn upsert_many(&self, profiles: &[MemberIdentityProfile]) -> Result<u64, MegaError> {
        let mut count = 0u64;
        for profile in profiles {
            self.upsert(profile.clone()).await?;
            count += 1;
        }
        Ok(count)
    }
}

fn non_empty_or(preferred: &str, fallback: &str) -> String {
    if !preferred.is_empty() {
        preferred.to_string()
    } else {
        fallback.to_string()
    }
}
