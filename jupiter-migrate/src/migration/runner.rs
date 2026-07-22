use common::errors::MegaError;
use sea_orm::DatabaseConnection;
use sea_orm_migration::prelude::MigratorTrait;
use tracing::log;

use super::Migrator;

/// Applies database migrations to the given database connection.
pub async fn apply_migrations(db: &DatabaseConnection, refresh: bool) -> Result<(), MegaError> {
    match refresh {
        true => Migrator::refresh(db).await,
        false => Migrator::up(db, None).await,
    }
    .map_err(|e| {
        log::error!("Failed to apply migrations: {e}");
        e.into()
    })
}

#[cfg(test)]
mod tests {
    use callisto::{
        email_jobs, notification_event_types, user_notification_preferences,
        user_notification_settings,
    };
    use sea_orm::{
        ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DbBackend, Set, Statement,
    };
    use sea_orm_migration::prelude::MigratorTrait;

    use super::*;

    async fn test_db_connection(temp_dir: &std::path::Path) -> DatabaseConnection {
        let db_url = format!("sqlite://{}/test.db", temp_dir.to_string_lossy());
        std::fs::File::create(temp_dir.join("test.db")).expect("create test db file");
        let mut opt = ConnectOptions::new(db_url);
        opt.max_connections(5).min_connections(1);
        Database::connect(opt).await.expect("connect test database")
    }

    #[tokio::test]
    async fn test_apply_migrations() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temporary directory");
        let db = test_db_connection(temp_dir.path()).await;
        let result = apply_migrations(&db, false).await;
        assert!(
            result.is_ok(),
            "Failed to apply migrations: {:?}",
            result.err()
        );

        let applied_migrations = Migrator::get_applied_migrations(&db).await.unwrap();
        assert!(!applied_migrations.is_empty(), "No migrations were applied");
    }

    #[tokio::test]
    async fn test_notification_center_schema_and_constraints() {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temporary directory");
        let db = test_db_connection(temp_dir.path()).await;

        apply_migrations(&db, true)
            .await
            .expect("migrations should apply");

        db.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;",
        ))
        .await
        .expect("enable sqlite foreign_keys");

        for table in [
            "notification_event_types",
            "user_notification_settings",
            "user_notification_preferences",
            "email_jobs",
        ] {
            let stmt = Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='{}' LIMIT 1;",
                    table
                ),
            );
            let row = db.query_one_raw(stmt).await.expect("query sqlite_master");
            assert!(row.is_some(), "expected table '{table}' to exist");
        }

        let now = chrono::Utc::now().naive_utc();

        notification_event_types::ActiveModel {
            code: Set("cl.comment.created".to_owned()),
            category: Set("cl".to_owned()),
            description: Set("New comment on a CL".to_owned()),
            system_required: Set(false),
            default_enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert event type");

        user_notification_settings::ActiveModel {
            username: Set("alice".to_owned()),
            email: Set("alice@example.com".to_owned()),
            enabled: Set(true),
            delivery_mode: Set("realtime".to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert user settings");

        user_notification_preferences::ActiveModel {
            username: Set("alice".to_owned()),
            event_type_code: Set("cl.comment.created".to_owned()),
            enabled: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert user preference");

        let res = user_notification_preferences::ActiveModel {
            username: Set("alice".to_owned()),
            event_type_code: Set("does.not.exist".to_owned()),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await;
        assert!(res.is_err(), "expected FK violation for unknown event type");

        email_jobs::ActiveModel {
            id: Default::default(),
            username: Set("alice".to_owned()),
            to_email: Set("alice@example.com".to_owned()),
            event_type_code: Set("cl.comment.created".to_owned()),
            subject: Set("Test".to_owned()),
            body_html: Set("<p>Hello</p>".to_owned()),
            body_text: Set(Some("Hello".to_owned())),
            status: Set("pending".to_owned()),
            error_message: Set(None),
            retry_count: Set(0),
            next_retry_at: Set(None),
            sent_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await
        .expect("insert email job");

        let res = email_jobs::ActiveModel {
            id: Default::default(),
            username: Set("alice".to_owned()),
            to_email: Set("alice@example.com".to_owned()),
            event_type_code: Set("does.not.exist".to_owned()),
            subject: Set("Test".to_owned()),
            body_html: Set("<p>Hello</p>".to_owned()),
            body_text: Set(None),
            status: Set("pending".to_owned()),
            error_message: Set(None),
            retry_count: Set(0),
            next_retry_at: Set(None),
            sent_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&db)
        .await;
        assert!(res.is_err(), "expected FK violation for unknown event type");
    }
}
