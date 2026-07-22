use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const RENAMES: &[(&str, &str)] = &[
    ("cl.created", "cl_created"),
    ("cl.updated", "cl_updated"),
    ("cl.merged", "cl_merged"),
    ("cl.closed", "cl_closed"),
    ("cl.reopened", "cl_reopened"),
    ("cl.comment.created", "cl_comment_created"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        match manager.get_database_backend() {
            DatabaseBackend::Postgres => {
                // Idempotent: only rename labels that still use the dotted form
                // (fresh installs already create underscore labels in m20260324).
                for (from, to) in RENAMES {
                    conn.execute_raw(Statement::from_string(
                        DatabaseBackend::Postgres,
                        format!(
                            r#"DO $$ BEGIN
  IF EXISTS (
    SELECT 1
    FROM pg_enum e
    JOIN pg_type t ON e.enumtypid = t.oid
    WHERE t.typname = 'webhook_event_type_enum'
      AND e.enumlabel = '{from}'
  ) THEN
    ALTER TYPE webhook_event_type_enum RENAME VALUE '{from}' TO '{to}';
  END IF;
END $$;"#
                        ),
                    ))
                    .await?;
                }
            }
            DatabaseBackend::Sqlite => {
                for (from, to) in RENAMES {
                    for table in ["mega_webhook_event_type", "mega_webhook_delivery"] {
                        conn.execute_unprepared(&format!(
                            "UPDATE {table} SET event_type = '{to}' WHERE event_type = '{from}'"
                        ))
                        .await?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        match manager.get_database_backend() {
            DatabaseBackend::Postgres => {
                for (from, to) in RENAMES {
                    conn.execute_raw(Statement::from_string(
                        DatabaseBackend::Postgres,
                        format!(
                            r#"DO $$ BEGIN
  IF EXISTS (
    SELECT 1
    FROM pg_enum e
    JOIN pg_type t ON e.enumtypid = t.oid
    WHERE t.typname = 'webhook_event_type_enum'
      AND e.enumlabel = '{to}'
  ) THEN
    ALTER TYPE webhook_event_type_enum RENAME VALUE '{to}' TO '{from}';
  END IF;
END $$;"#
                        ),
                    ))
                    .await?;
                }
            }
            DatabaseBackend::Sqlite => {
                for (from, to) in RENAMES {
                    for table in ["mega_webhook_event_type", "mega_webhook_delivery"] {
                        conn.execute_unprepared(&format!(
                            "UPDATE {table} SET event_type = '{from}' WHERE event_type = '{to}'"
                        ))
                        .await?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}
