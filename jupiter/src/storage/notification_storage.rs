use std::sync::Arc;

use callisto::{
    email_jobs, notification_event_types, user_notification_preferences, user_notification_settings,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, sea_query::Expr,
};

#[derive(Clone)]
pub struct NotificationStorage {
    db: Arc<DatabaseConnection>,
}

impl NotificationStorage {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    // Evnt types
    pub async fn list_event_types(
        &self,
    ) -> Result<Vec<notification_event_types::Model>, sea_orm::DbErr> {
        notification_event_types::Entity::find()
            .all(self.db())
            .await
    }

    pub async fn get_event_type(
        &self,
        code: &str,
    ) -> Result<Option<notification_event_types::Model>, sea_orm::DbErr> {
        notification_event_types::Entity::find()
            .filter(notification_event_types::Column::Code.eq(code))
            .one(self.db())
            .await
    }

    //User notification settings
    pub async fn get_user_settings(
        &self,
        username: &str,
    ) -> Result<Option<user_notification_settings::Model>, sea_orm::DbErr> {
        user_notification_settings::Entity::find()
            .filter(user_notification_settings::Column::CampsiteUserId.eq(username))
            .one(self.db())
            .await
    }

    pub async fn upsert_user_settings(
        &self,
        username: &str,
        email: &str,
    ) -> Result<(), sea_orm::DbErr> {
        let now = chrono::Utc::now().naive_utc();

        if let Some(existing) = self.get_user_settings(username).await? {
            let mut model: user_notification_settings::ActiveModel = existing.into();
            model.email = Set(email.to_string());
            model.updated_at = Set(now);
            model.update(self.db()).await?;
        } else {
            user_notification_settings::ActiveModel {
                campsite_user_id: Set(username.to_string()),
                email: Set(email.to_string()),
                enabled: Set(true),
                delivery_mode: Set("realtime".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(self.db())
            .await?;
        }

        Ok(())
    }

    // Notification preferences
    pub async fn get_user_preference(
        &self,
        username: &str,
        event_type_code: &str,
    ) -> Result<Option<user_notification_preferences::Model>, sea_orm::DbErr> {
        user_notification_preferences::Entity::find()
            .filter(user_notification_preferences::Column::CampsiteUserId.eq(username))
            .filter(user_notification_preferences::Column::EventTypeCode.eq(event_type_code))
            .one(self.db())
            .await
    }

    pub async fn set_user_preference(
        &self,
        username: &str,
        event_type_code: &str,
        enabled: bool,
    ) -> Result<(), sea_orm::DbErr> {
        let now = chrono::Utc::now().naive_utc();

        if let Some(existing) = self.get_user_preference(username, event_type_code).await? {
            let mut model: user_notification_preferences::ActiveModel = existing.into();
            model.enabled = Set(enabled);
            model.updated_at = Set(now);
            model.update(self.db()).await?;
        } else {
            user_notification_preferences::ActiveModel {
                campsite_user_id: Set(username.to_string()),
                event_type_code: Set(event_type_code.to_string()),
                enabled: Set(enabled),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(self.db())
            .await?;
        }

        Ok(())
    }

    pub async fn list_user_preferences(
        &self,
        username: &str,
    ) -> Result<Vec<user_notification_preferences::Model>, sea_orm::DbErr> {
        user_notification_preferences::Entity::find()
            .filter(user_notification_preferences::Column::CampsiteUserId.eq(username))
            .all(self.db())
            .await
    }

    pub async fn set_global_enabled(
        &self,
        username: &str,
        enabled: bool,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(existing) = self.get_user_settings(username).await? {
            let mut model: user_notification_settings::ActiveModel = existing.into();
            model.enabled = Set(enabled);
            model.updated_at = Set(chrono::Utc::now().naive_utc());
            model.update(self.db()).await?;
        }
        Ok(())
    }

    pub async fn set_delivery_mode(
        &self,
        username: &str,
        mode: &str,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(existing) = self.get_user_settings(username).await? {
            let mut model: user_notification_settings::ActiveModel = existing.into();
            model.delivery_mode = Set(mode.to_string());
            model.updated_at = Set(chrono::Utc::now().naive_utc());
            model.update(self.db()).await?;
        }
        Ok(())
    }

    // Main logic of whether to send a notification for a given user and event type
    pub async fn should_send(
        &self,
        username: &str,
        event_type_code: &str,
    ) -> Result<bool, sea_orm::DbErr> {
        let event_type = match self.get_event_type(event_type_code).await? {
            Some(e) => e,
            None => return Ok(false),
        };

        let settings = match self.get_user_settings(username).await? {
            Some(s) => s,
            None => return Ok(false),
        };

        if !settings.enabled {
            return Ok(false);
        }

        if event_type.system_required {
            return Ok(true);
        }

        if let Some(pref) = self.get_user_preference(username, event_type_code).await? {
            return Ok(pref.enabled);
        }

        Ok(event_type.default_enabled)
    }

    // Email job management
    pub async fn enqueue_email_job(
        &self,
        username: &str,
        to_email: &str,
        event_type_code: &str,
        subject: &str,
        body_html: &str,
        body_text: Option<&str>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = chrono::Utc::now().naive_utc();

        email_jobs::ActiveModel {
            id: Default::default(),
            campsite_user_id: Set(username.to_string()),
            to_email: Set(to_email.to_string()),
            event_type_code: Set(event_type_code.to_string()),
            subject: Set(subject.to_string()),
            body_html: Set(body_html.to_string()),
            body_text: Set(body_text.map(|s| s.to_string())),
            status: Set("pending".to_string()),
            error_message: Set(None),
            retry_count: Set(0),
            next_retry_at: Set(None),
            sent_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(self.db())
        .await?;

        Ok(())
    }

    /// Fetch pending email jobs that are ready to be sent
    /// Jobs are eligible when:
    /// - status == "pending"
    /// - next_retry_at is NULL, or next_retry_at <= now
    pub async fn fetch_pending_jobs(
        &self,
        limit: u64,
    ) -> Result<Vec<email_jobs::Model>, sea_orm::DbErr> {
        let now = chrono::Utc::now().naive_utc();

        email_jobs::Entity::find()
            .filter(email_jobs::Column::Status.eq("pending"))
            .filter(
                Condition::any()
                    .add(email_jobs::Column::NextRetryAt.is_null())
                    .add(email_jobs::Column::NextRetryAt.lte(now)),
            )
            .order_by_asc(email_jobs::Column::CreatedAt)
            .limit(limit)
            .all(self.db())
            .await
    }

    /// Try to claim a job for sending by atomically changing its status from "pending" to "sending".
    /// Returns true if the claim was successful, false if the job was already claimed by another
    pub async fn try_claim_job(&self, job_id: i64) -> Result<bool, sea_orm::DbErr> {
        let now = chrono::Utc::now().naive_utc();
        let res = email_jobs::Entity::update_many()
            .col_expr(email_jobs::Column::Status, Expr::value("sending"))
            .col_expr(email_jobs::Column::UpdatedAt, Expr::value(now))
            .filter(email_jobs::Column::Id.eq(job_id))
            .filter(email_jobs::Column::Status.eq("pending"))
            .exec(self.db())
            .await?;
        Ok(res.rows_affected == 1)
    }

    pub async fn mark_job_sent(&self, job_id: i64) -> Result<(), sea_orm::DbErr> {
        if let Some(job) = email_jobs::Entity::find_by_id(job_id)
            .one(self.db())
            .await?
        {
            let mut model: email_jobs::ActiveModel = job.into();
            let now = chrono::Utc::now().naive_utc();

            model.status = Set("sent".to_string());
            model.error_message = Set(None);
            model.sent_at = Set(Some(now));
            model.updated_at = Set(now);
            model.update(self.db()).await?;
        }
        Ok(())
    }

    pub async fn mark_job_skipped(&self, job_id: i64, reason: &str) -> Result<(), sea_orm::DbErr> {
        if let Some(job) = email_jobs::Entity::find_by_id(job_id)
            .one(self.db())
            .await?
        {
            let mut model: email_jobs::ActiveModel = job.into();
            let now = chrono::Utc::now().naive_utc();

            model.status = Set("skipped".to_string());
            model.error_message = Set(Some(reason.to_string()));
            model.updated_at = Set(now);
            model.update(self.db()).await?;
        }
        Ok(())
    }

    /// Mark a job as failed and schedule a retry
    /// backoff: 30s, 60s, ... capped at 300s
    pub async fn mark_job_failed_with_retry(
        &self,
        job_id: i64,
        error: &str,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(job) = email_jobs::Entity::find_by_id(job_id)
            .one(self.db())
            .await?
        {
            let mut model: email_jobs::ActiveModel = job.clone().into();
            let now = chrono::Utc::now().naive_utc();
            let retry = job.retry_count + 1;
            let delay_secs = (retry as i64).min(10) * 30;
            let next = now + chrono::Duration::seconds(delay_secs);

            // Re-queue
            model.status = Set("pending".to_string());
            model.error_message = Set(Some(error.to_string()));
            model.retry_count = Set(retry);
            model.next_retry_at = Set(Some(next));
            model.updated_at = Set(now);
            model.update(self.db()).await?;
        }
        Ok(())
    }

    pub async fn mark_job_failed(&self, job_id: i64, error: &str) -> Result<(), sea_orm::DbErr> {
        self.mark_job_failed_with_retry(job_id, error).await
    }
}

#[cfg(test)]
mod tests {
    use callisto::notification_event_types;
    use jupiter_migrate::apply_migrations;
    use sea_orm::{ActiveModelTrait, Set};

    use super::*;
    use crate::tests::test_db_connection;

    #[tokio::test]
    async fn test_should_send_logic() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db = test_db_connection(temp_dir.path()).await;

        apply_migrations(&db, true).await.unwrap();

        let storage = NotificationStorage::new(Arc::new(db.clone()));

        let now = chrono::Utc::now().naive_utc();

        // Insert event type
        notification_event_types::ActiveModel {
            code: Set("test.event".to_string()),
            category: Set("test".to_string()),
            description: Set("desc".to_string()),
            system_required: Set(false),
            default_enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        // Insert user settings
        storage
            .upsert_user_settings("alice", "alice@test.com")
            .await
            .unwrap();

        // No override → default_enabled = true
        assert!(storage.should_send("alice", "test.event").await.unwrap());

        // Override false
        storage
            .set_user_preference("alice", "test.event", false)
            .await
            .unwrap();

        assert!(!storage.should_send("alice", "test.event").await.unwrap());
    }

    #[tokio::test]
    async fn test_enqueue_and_mark_job() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db = test_db_connection(temp_dir.path()).await;

        apply_migrations(&db, true).await.unwrap();

        let storage = NotificationStorage::new(Arc::new(db.clone()));
        let now = chrono::Utc::now().naive_utc();

        // Insert event type + user
        notification_event_types::ActiveModel {
            code: Set("test.event".to_string()),
            category: Set("test".to_string()),
            description: Set("desc".to_string()),
            system_required: Set(false),
            default_enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        storage
            .upsert_user_settings("alice", "alice@test.com")
            .await
            .unwrap();

        // Enqueue
        storage
            .enqueue_email_job(
                "alice",
                "alice@test.com",
                "test.event",
                "Hello",
                "<p>Hello</p>",
                Some("Hello"),
            )
            .await
            .unwrap();

        let jobs = storage.fetch_pending_jobs(10).await.unwrap();
        assert_eq!(jobs.len(), 1);

        let job_id = jobs[0].id;

        // Mark sent
        storage.mark_job_sent(job_id).await.unwrap();

        let job = callisto::email_jobs::Entity::find_by_id(job_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(job.status, "sent");
    }
}
