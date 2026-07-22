use sea_orm_migration::{prelude::*, sea_orm::DatabaseBackend};

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
                            .table(MegaConversation::Table)
                            .modify_column(
                                ColumnDef::new(Alias::new("user_id"))
                                    .string()
                                    .not_null()
                                    .to_owned(),
                            )
                            .to_owned(),
                    )
                    .await?;

                manager
                    .alter_table(
                        Table::alter()
                            .table(AccessToken::Table)
                            .modify_column(
                                ColumnDef::new(Alias::new("user_id"))
                                    .string()
                                    .not_null()
                                    .to_owned(),
                            )
                            .to_owned(),
                    )
                    .await?;

                manager
                    .alter_table(
                        Table::alter()
                            .table(SshKeys::Table)
                            .modify_column(
                                ColumnDef::new(Alias::new("user_id"))
                                    .string()
                                    .not_null()
                                    .to_owned(),
                            )
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
enum MegaConversation {
    Table,
}

#[derive(DeriveIden)]
enum SshKeys {
    Table,
}

#[derive(DeriveIden)]
enum AccessToken {
    Table,
}
