use sea_orm::DatabaseBackend;
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"ALTER TYPE conv_type_enum ADD VALUE IF NOT EXISTS 'assignee';"#,
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        manager
            .create_table(
                table_auto(ItemAssignees::Table)
                    .col(big_integer(ItemAssignees::ItemId))
                    .col(string(ItemAssignees::AssignneeId))
                    .col(string(ItemAssignees::ItemType))
                    .primary_key(
                        Index::create()
                            .col(ItemAssignees::ItemId)
                            .col(ItemAssignees::AssignneeId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaIssue::Table)
                    .drop_column("user_id")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaIssue::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(Alias::new("author"))
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ItemAssignees {
    Table,
    ItemId,
    AssignneeId,
    ItemType,
}

#[derive(DeriveIden)]
enum MegaIssue {
    Table,
}
