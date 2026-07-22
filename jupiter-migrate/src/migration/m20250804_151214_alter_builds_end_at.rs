use sea_orm_migration::{prelude::*, schema::*, sea_orm::DatabaseBackend};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                // ALTER TABLE builds ALTER COLUMN end_at DROP NOT NULL
                manager
                    .alter_table(
                        Table::alter()
                            .table(Builds::Table)
                            .modify_column(timestamp_null(Builds::EndAt).to_owned())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                // Reverting the change - making end_at NOT NULL again
                // Note: This could fail if there are NULL values in the column
                manager
                    .alter_table(
                        Table::alter()
                            .table(Builds::Table)
                            .modify_column(timestamp(Builds::EndAt).to_owned())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    EndAt,
}
