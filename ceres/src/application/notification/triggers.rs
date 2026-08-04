use std::collections::HashSet;

use callisto::notification_event_types;
use common::errors::MegaError;
use jupiter::{
    sea_orm::{ActiveModelTrait, Set},
    storage::{
        cl_reviewer_storage::ClReviewerStorage, cl_storage::ClStorage,
        notification_storage::NotificationStorage,
    },
};

pub const EVENT_CL_COMMENT_CREATED: &str = "cl.comment.created";

/// Ensures the CL comment notification event type exists (idempotent).
pub async fn ensure_cl_comment_event_type(stg: &NotificationStorage) -> Result<(), MegaError> {
    if stg
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
    .insert(stg.db())
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

/// Trigger: a new comment is created on a Change List.
pub async fn on_cl_comment_created(
    notif_stg: &NotificationStorage,
    cl_stg: &ClStorage,
    reviewer_stg: &ClReviewerStorage,
    actor_username: &str,
    cl_link: &str,
    comment_text: &str,
) -> Result<(), MegaError> {
    ensure_cl_comment_event_type(notif_stg).await?;

    let cl = cl_stg
        .get_cl(cl_link)
        .await?
        .ok_or_else(|| MegaError::NotFound(format!("CL {cl_link} not found")))?;

    let reviewers = reviewer_stg.list_reviewers(cl_link).await?;

    let mut recipients: HashSet<String> = HashSet::new();
    recipients.insert(cl.campsite_user_id);
    for r in reviewers {
        recipients.insert(r.campsite_user_id);
    }
    recipients.remove(actor_username);

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
        let body_text = format!("{actor_username} commented on {cl_link}: {comment_text}");
        let body_html = format!(
            "<p><b>{actor_username}</b> commented on <b>{cl_link}</b>:</p><p>{}</p>",
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use callisto::{email_jobs, mega_cl, mega_cl_reviewer};
    use jupiter::{
        sea_orm::{ColumnTrait, EntityTrait, QueryFilter},
        storage::base_storage::{BaseStorage, StorageConnector},
        tests::test_db_connection,
    };
    use jupiter_migrate::apply_migrations;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_on_cl_comment_created_enqueues_jobs_for_author_and_reviewers() {
        let dir = TempDir::new().unwrap();
        let db = test_db_connection(dir.path()).await;
        apply_migrations(&db, true).await.unwrap();

        let base = BaseStorage::new(Arc::new(db.clone()));
        let notif = NotificationStorage::new(Arc::new(db.clone()));
        let cl_stg = ClStorage { base: base.clone() };
        let reviewer_stg = ClReviewerStorage { base: base.clone() };

        let now = chrono::Utc::now().naive_utc();
        mega_cl::ActiveModel {
            id: Set(1),
            link: Set("CL1".to_string()),
            title: Set("t".to_string()),
            merge_date: Set(None),
            status: Set(callisto::sea_orm_active_enums::MergeStatusEnum::Open),
            path: Set("/".to_string()),
            from_hash: Set("a".to_string()),
            to_hash: Set("b".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            campsite_user_id: Set("alice".to_string()),
            base_branch: Set("main".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();

        mega_cl_reviewer::ActiveModel {
            id: Set(1),
            cl_link: Set("CL1".to_string()),
            campsite_user_id: Set("bob".to_string()),
            github_login: Set(Some("bob".to_string())),
            approved: Set(false),
            system_required: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();

        notif
            .upsert_user_settings("alice", "alice@example.com")
            .await
            .unwrap();
        notif
            .upsert_user_settings("bob", "bob@example.com")
            .await
            .unwrap();
        notif
            .upsert_user_settings("carol", "carol@example.com")
            .await
            .unwrap();

        on_cl_comment_created(&notif, &cl_stg, &reviewer_stg, "carol", "CL1", "hello")
            .await
            .unwrap();

        let jobs = email_jobs::Entity::find().all(&db).await.unwrap();
        assert_eq!(jobs.len(), 2);

        let alice_job = email_jobs::Entity::find()
            .filter(email_jobs::Column::CampsiteUserId.eq("alice"))
            .one(&db)
            .await
            .unwrap();
        assert!(alice_job.is_some());

        let bob_job = email_jobs::Entity::find()
            .filter(email_jobs::Column::CampsiteUserId.eq("bob"))
            .one(&db)
            .await
            .unwrap();
        assert!(bob_job.is_some());
    }

    #[tokio::test]
    async fn test_on_cl_comment_created_respects_should_send() {
        let dir = TempDir::new().unwrap();
        let db = test_db_connection(dir.path()).await;
        apply_migrations(&db, true).await.unwrap();

        let base = BaseStorage::new(Arc::new(db.clone()));
        let notif = NotificationStorage::new(Arc::new(db.clone()));
        let cl_stg = ClStorage { base: base.clone() };
        let reviewer_stg = ClReviewerStorage { base: base.clone() };
        let now = chrono::Utc::now().naive_utc();

        mega_cl::ActiveModel {
            id: Set(1),
            link: Set("CL2".to_string()),
            title: Set("t".to_string()),
            merge_date: Set(None),
            status: Set(callisto::sea_orm_active_enums::MergeStatusEnum::Open),
            path: Set("/".to_string()),
            from_hash: Set("a".to_string()),
            to_hash: Set("b".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            campsite_user_id: Set("alice".to_string()),
            base_branch: Set("main".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();

        notif
            .upsert_user_settings("alice", "alice@example.com")
            .await
            .unwrap();
        notif.set_global_enabled("alice", false).await.unwrap();

        on_cl_comment_created(&notif, &cl_stg, &reviewer_stg, "bob", "CL2", "hello")
            .await
            .unwrap();

        let jobs = email_jobs::Entity::find().all(&db).await.unwrap();
        assert_eq!(jobs.len(), 0);
    }
}
