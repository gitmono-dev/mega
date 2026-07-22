use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Sqlite => {
                // Add username column to mega_cl table for SQLite
                // This migration fixes the issue where m20250812_022434_alter_mega_mr
                // skipped adding username column for SQLite, and the table was later
                // renamed from mega_mr to mega_cl in m20250930_024736_mr_to_cl
                manager
                    .alter_table(
                        Table::alter()
                            .table(MegaCl::Table)
                            .add_column_if_not_exists(
                                ColumnDef::new(Alias::new("username"))
                                    .string()
                                    .not_null()
                                    .default(""),
                            )
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::Postgres | DatabaseBackend::MySql => {
                // Already handled by m20250812_022434_alter_mega_mr
                // No action needed
            }
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum MegaCl {
    Table,
}
