use sea_orm::DatabaseBackend;
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres | DatabaseBackend::MySql => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Builds::Table)
                            .drop_column("output")
                            .add_column_if_not_exists(text(Builds::OutputFile))
                            .add_column_if_not_exists(text(Builds::Arguments))
                            .add_column_if_not_exists(text(Builds::Mr))
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    OutputFile,
    Arguments,
    Mr,
}
