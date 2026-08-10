use std::{collections::HashMap, ops::Deref};

use callisto::{access_token, entity_ext::generate_id, mega_cl_reviewer};
use common::errors::MegaError;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, QueryFilter, Set,
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone)]
pub struct ClReviewerStorage {
    pub base: BaseStorage,
}

impl Deref for ClReviewerStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ClReviewerStorage {
    pub fn new_reviewer(
        &self,
        cl_link: &str,
        campsite_user_id: &str,
        github_login: Option<String>,
    ) -> mega_cl_reviewer::Model {
        let now = chrono::Utc::now().naive_utc();
        mega_cl_reviewer::Model {
            id: generate_id(),
            cl_link: cl_link.to_string(),
            approved: false,
            campsite_user_id: campsite_user_id.to_string(),
            github_login,
            created_at: now,
            updated_at: now,
            system_required: false,
        }
    }

    pub async fn add_reviewers(
        &self,
        cl_link: &str,
        reviewers: Vec<(String, Option<String>)>,
    ) -> Result<(), MegaError> {
        for (campsite_user_id, github_login) in reviewers {
            let new_reviewer = self.new_reviewer(cl_link, &campsite_user_id, github_login);
            let a_model: mega_cl_reviewer::ActiveModel = new_reviewer.into_active_model();
            a_model.insert(self.get_connection()).await.map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!("reviewer {}", campsite_user_id.clone()))
            })?;
        }
        Ok(())
    }

    pub async fn update_system_required_reviewers(
        &self,
        cl_link: &str,
        campsite_user_ids: &[String],
        system_required: bool,
    ) -> Result<(), MegaError> {
        for campsite_user_id in campsite_user_ids {
            let mut rev: mega_cl_reviewer::ActiveModel = mega_cl_reviewer::Entity::find()
                .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
                .filter(mega_cl_reviewer::Column::CampsiteUserId.eq(campsite_user_id))
                .one(self.get_connection())
                .await
                .map_err(|e| {
                    tracing::error!("{}", e);
                    MegaError::Other(format!("fail to find reviewer {}", campsite_user_id))
                })?
                .ok_or_else(|| {
                    MegaError::NotFound(format!("reviewer {} not found", campsite_user_id))
                })?
                .into_active_model();

            rev.system_required = Set(system_required);
            rev.updated_at = Set(chrono::Utc::now().naive_utc());
            rev.update(self.get_connection()).await.map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!(
                    "fail to update system required for reviewer {}",
                    campsite_user_id
                ))
            })?;
        }
        Ok(())
    }

    pub async fn remove_reviewers(
        &self,
        cl_link: &str,
        campsite_user_ids: &[String],
    ) -> Result<(), MegaError> {
        if campsite_user_ids.is_empty() {
            return Ok(());
        }
        mega_cl_reviewer::Entity::delete_many()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::CampsiteUserId.is_in(campsite_user_ids.to_vec()))
            .filter(mega_cl_reviewer::Column::SystemRequired.eq(false))
            .exec(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!("fail to remove reviewers: {:?}", campsite_user_ids))
            })?;
        Ok(())
    }

    pub async fn remove_system_reviewers(
        &self,
        cl_link: &str,
        campsite_user_ids: &[String],
    ) -> Result<(), MegaError> {
        if campsite_user_ids.is_empty() {
            return Ok(());
        }
        mega_cl_reviewer::Entity::delete_many()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::CampsiteUserId.is_in(campsite_user_ids.to_vec()))
            .filter(mega_cl_reviewer::Column::SystemRequired.eq(true))
            .exec(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!(
                    "fail to remove system reviewers: {:?}",
                    campsite_user_ids
                ))
            })?;
        Ok(())
    }

    pub async fn is_reviewer(
        &self,
        cl_link: &str,
        campsite_user_id: &str,
    ) -> Result<bool, MegaError> {
        Ok(self
            .find_actor_reviewer(cl_link, campsite_user_id)
            .await?
            .is_some())
    }

    /// Find the reviewer row for an actor campsite public id.
    ///
    /// Also matches transitional rows keyed by github_login (or with
    /// `campsite_user_id == github_login`) and self-heals them to the public id.
    pub async fn find_actor_reviewer(
        &self,
        cl_link: &str,
        campsite_user_id: &str,
    ) -> Result<Option<mega_cl_reviewer::Model>, MegaError> {
        let campsite_user_id = campsite_user_id.trim();
        if campsite_user_id.is_empty() {
            return Ok(None);
        }

        if let Some(row) = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::CampsiteUserId.eq(campsite_user_id))
            .one(self.get_connection())
            .await?
        {
            return Ok(Some(row));
        }

        let github_login = self
            .github_login_for_campsite_user(campsite_user_id)
            .await?;
        let Some(login) = github_login else {
            return Ok(None);
        };

        let row = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(
                Condition::any()
                    .add(mega_cl_reviewer::Column::GithubLogin.eq(&login))
                    .add(mega_cl_reviewer::Column::CampsiteUserId.eq(&login)),
            )
            .one(self.get_connection())
            .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        // Self-heal transitional handle keys to campsite public id.
        if row.campsite_user_id != campsite_user_id {
            tracing::info!(
                cl_link,
                from = %row.campsite_user_id,
                to = %campsite_user_id,
                github_login = %login,
                "Remapping transitional CL reviewer campsite_user_id"
            );
            let mut am = row.into_active_model();
            am.campsite_user_id = Set(campsite_user_id.to_string());
            am.github_login = Set(Some(login));
            am.updated_at = Set(chrono::Utc::now().naive_utc());
            let updated = am.update(self.get_connection()).await?;
            return Ok(Some(updated));
        }

        Ok(Some(row))
    }

    async fn github_login_for_campsite_user(
        &self,
        campsite_user_id: &str,
    ) -> Result<Option<String>, MegaError> {
        let row = access_token::Entity::find()
            .filter(access_token::Column::CampsiteUserId.eq(campsite_user_id))
            .filter(access_token::Column::GithubLogin.is_not_null())
            .one(self.get_connection())
            .await?;
        Ok(row.and_then(|r| {
            r.github_login
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }))
    }

    pub async fn find_by_github_login(
        &self,
        cl_link: &str,
        github_login: &str,
    ) -> Result<Option<mega_cl_reviewer::Model>, MegaError> {
        mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::GithubLogin.eq(github_login))
            .one(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!(
                    "fail to find reviewer by github_login {}",
                    github_login
                ))
            })
    }

    pub async fn list_reviewers(
        &self,
        cl_link: &str,
    ) -> Result<Vec<mega_cl_reviewer::Model>, MegaError> {
        let reviewers = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .all(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!("fail to list reviewers for {cl_link}"))
            })?;
        Ok(reviewers)
    }

    /// Distinct `github_login → campsite_user_id` pairs already stored on reviewers.
    pub async fn github_login_to_campsite_ids(&self) -> Result<HashMap<String, String>, MegaError> {
        let rows = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::GithubLogin.is_not_null())
            .all(self.get_connection())
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            let Some(login) = row.github_login.as_deref().map(str::trim) else {
                continue;
            };
            if login.is_empty() || row.campsite_user_id.trim().is_empty() {
                continue;
            }
            // Prefer public-id shaped campsite_user_id when duplicates exist.
            let id = row.campsite_user_id.trim();
            if !map.contains_key(login)
                || (id.len() == 12 && id.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                map.insert(login.to_string(), id.to_string());
            }
        }
        Ok(map)
    }

    /// Remap reviewers still keyed by github_login (or equal to github_login) to
    /// campsite public ids using `login_to_id`. Idempotent; used when listing a CL
    /// so transitional rows heal without a manual SQL patch.
    pub async fn remap_transitional_reviewers(
        &self,
        cl_link: &str,
        login_to_id: &HashMap<String, String>,
    ) -> Result<u64, MegaError> {
        if login_to_id.is_empty() {
            return Ok(0);
        }
        let rows = self.list_reviewers(cl_link).await?;
        let mut updated = 0u64;
        for row in rows {
            let login_hint = row
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(row.campsite_user_id.trim())
                .to_string();
            let Some(target_id) = login_to_id.get(&login_hint).cloned() else {
                continue;
            };
            if target_id.is_empty() || row.campsite_user_id == target_id {
                continue;
            }
            // Only remap obvious transitional keys (handle stored as id).
            let is_transitional = row.campsite_user_id == login_hint
                || row
                    .github_login
                    .as_deref()
                    .is_some_and(|g| g == row.campsite_user_id);
            if !is_transitional {
                continue;
            }

            // Avoid unique collisions if a public-id row already exists.
            let exists = mega_cl_reviewer::Entity::find()
                .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
                .filter(mega_cl_reviewer::Column::CampsiteUserId.eq(&target_id))
                .one(self.get_connection())
                .await?
                .is_some();
            if exists {
                mega_cl_reviewer::Entity::delete_by_id(row.id)
                    .exec(self.get_connection())
                    .await?;
                updated += 1;
                continue;
            }

            let mut am = row.into_active_model();
            am.campsite_user_id = Set(target_id);
            am.github_login = Set(Some(login_hint));
            am.updated_at = Set(chrono::Utc::now().naive_utc());
            am.update(self.get_connection()).await?;
            updated += 1;
        }
        Ok(updated)
    }

    pub async fn reviewer_change_state(
        &self,
        cl_link: &str,
        campsite_user_id: &str,
        approved: bool,
    ) -> Result<(), MegaError> {
        let row = self
            .find_actor_reviewer(cl_link, campsite_user_id)
            .await?
            .ok_or_else(|| {
                MegaError::NotFound(format!("reviewer {} not found", campsite_user_id))
            })?;

        let mut rev: mega_cl_reviewer::ActiveModel = row.into_active_model();
        rev.approved = Set(approved);
        rev.updated_at = Set(chrono::Utc::now().naive_utc());
        rev.update(self.get_connection()).await.map_err(|e| {
            tracing::error!("{}", e);
            MegaError::Other(format!("fail to update reviewer {}", campsite_user_id))
        })?;

        Ok(())
    }
}
