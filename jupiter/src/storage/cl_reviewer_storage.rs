use std::ops::Deref;

use callisto::{entity_ext::generate_id, mega_cl_reviewer};
use common::errors::MegaError;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};

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
                    MegaError::Other(format!("reviewer {} not found", campsite_user_id))
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
        let is_reviewer = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::CampsiteUserId.eq(campsite_user_id))
            .one(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("Error finding the reviewer: {}", e);
                e
            })?
            .is_some();

        Ok(is_reviewer)
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

    pub async fn reviewer_change_state(
        &self,
        cl_link: &str,
        campsite_user_id: &str,
        approved: bool,
    ) -> Result<(), MegaError> {
        let mut rev: mega_cl_reviewer::ActiveModel = mega_cl_reviewer::Entity::find()
            .filter(mega_cl_reviewer::Column::ClLink.eq(cl_link))
            .filter(mega_cl_reviewer::Column::CampsiteUserId.eq(campsite_user_id))
            .one(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{}", e);
                MegaError::Other(format!("fail to find reviewer {}", campsite_user_id))
            })?
            .ok_or_else(|| MegaError::Other(format!("reviewer {} not found", campsite_user_id)))?
            .into_active_model();

        rev.approved = Set(approved);
        rev.updated_at = Set(chrono::Utc::now().naive_utc());
        rev.update(self.get_connection()).await.map_err(|e| {
            tracing::error!("{}", e);
            MegaError::Other(format!("fail to update reviewer {}", campsite_user_id))
        })?;

        Ok(())
    }
}
