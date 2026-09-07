//! Align PostgreSQL columns with CLI-generated DateTimeUtc source entities.
//! Keep this as a forward migration: an earlier draft source schema may already
//! have been applied. Existing source code wrote UTC values into these columns.

use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DbBackend::Postgres {
            return Ok(());
        }
        for table in [
            "snapshot_instance",
            "snapshot_source",
            "source_commit_scope",
        ] {
            manager.get_connection().execute_unprepared(&format!(
                "ALTER TABLE {table} ALTER COLUMN created_at TYPE TIMESTAMP WITH TIME ZONE USING created_at AT TIME ZONE 'UTC'"
            )).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DbBackend::Postgres {
            return Ok(());
        }
        for table in [
            "snapshot_instance",
            "snapshot_source",
            "source_commit_scope",
        ] {
            manager.get_connection().execute_unprepared(&format!(
                "ALTER TABLE {table} ALTER COLUMN created_at TYPE TIMESTAMP WITHOUT TIME ZONE USING created_at AT TIME ZONE 'UTC'"
            )).await?;
        }
        Ok(())
    }
}
