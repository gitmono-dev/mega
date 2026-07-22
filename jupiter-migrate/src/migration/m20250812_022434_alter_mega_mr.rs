use sea_orm::{DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

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
                            .table(MegaMr::Table)
                            .add_column_if_not_exists(
                                ColumnDef::new(Alias::new("username"))
                                    .string()
                                    .not_null()
                                    .default(""),
                            )
                            .to_owned(),
                    )
                    .await?;

                manager
                    .get_connection()
                    .execute_raw(Statement::from_string(
                        manager.get_database_backend(),
                        "ALTER TABLE access_token RENAME COLUMN user_id TO username".to_owned(),
                    ))
                    .await?;

                manager
                    .get_connection()
                    .execute_raw(Statement::from_string(
                        manager.get_database_backend(),
                        "ALTER TABLE ssh_keys RENAME COLUMN user_id TO username".to_owned(),
                    ))
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
enum MegaMr {
    Table,
}

// #[derive(DeriveIden)]
// enum AccessToken {
//     Table,
// }

// #[derive(DeriveIden)]
// enum SshKeys {
//     Table,
// }
