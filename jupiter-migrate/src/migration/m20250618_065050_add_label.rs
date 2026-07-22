use sea_orm_migration::{prelude::*, schema::*, sea_orm::DatabaseBackend};

use crate::migration::pk_bigint;

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
                        r#"ALTER TYPE conv_type_enum ADD VALUE IF NOT EXISTS 'label';"#,
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        manager
            .create_table(
                table_auto(Label::Table)
                    .col(pk_bigint(Label::Id))
                    .col(string(Label::Name))
                    .col(string(Label::Color))
                    .col(string(Label::Description))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                table_auto(ItemLabels::Table)
                    .col(big_integer(ItemLabels::ItemId))
                    .col(big_integer(ItemLabels::LabelId))
                    .col(string(ItemLabels::ItemType))
                    .primary_key(
                        Index::create()
                            .col(ItemLabels::ItemId)
                            .col(ItemLabels::LabelId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaIssue::Table)
                    .drop_column("owner")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaIssue::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(Alias::new("user_id"))
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
enum Label {
    Table,
    Id,
    Name,
    Color,
    Description,
}

#[derive(DeriveIden)]
enum ItemLabels {
    Table,
    ItemId,
    LabelId,
    ItemType,
}

#[derive(DeriveIden)]
enum MegaIssue {
    Table,
}
