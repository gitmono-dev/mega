use common::errors::MegaError;

use super::context::ReviewerApplicationService;
use crate::model::change_list::{ReviewerInfo, ReviewersResponse};

impl ReviewerApplicationService {
    /// `reviewers` are `(campsite_user_id, github_login)` pairs.
    /// Mono may pass `(id, None)` when the frontend only has campsite ids.
    pub async fn add_reviewers(
        &self,
        link: &str,
        reviewers: Vec<(String, Option<String>)>,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .reviewer_storage()
            .add_reviewers(link, reviewers)
            .await
    }

    pub async fn remove_reviewers(
        &self,
        link: &str,
        reviewers: &[String],
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .reviewer_storage()
            .remove_reviewers(link, reviewers)
            .await
    }

    pub async fn list_reviewers(&self, link: &str) -> Result<ReviewersResponse, MegaError> {
        let reviewers = self
            .ctx
            .storage()
            .reviewer_storage()
            .list_reviewers(link)
            .await?
            .into_iter()
            .map(|r| ReviewerInfo {
                campsite_user_id: r.campsite_user_id.clone(),
                github_login: r.github_login.clone(),
                username: r
                    .github_login
                    .clone()
                    .unwrap_or_else(|| r.campsite_user_id.clone()),
                approved: r.approved,
                system_required: r.system_required,
            })
            .collect();
        Ok(ReviewersResponse { result: reviewers })
    }

    pub async fn reviewer_change_state(
        &self,
        link: &str,
        campsite_user_id: &str,
        approved: bool,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .reviewer_storage()
            .reviewer_change_state(link, campsite_user_id, approved)
            .await
    }

    pub async fn is_reviewer(&self, link: &str, campsite_user_id: &str) -> Result<bool, MegaError> {
        self.ctx
            .storage()
            .reviewer_storage()
            .is_reviewer(link, campsite_user_id)
            .await
    }
}
