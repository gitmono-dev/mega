use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MegaCl::Table)
                    .add_column_if_not_exists(ColumnDef::new(MegaCl::ClosedAt).date_time().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MegaCl::Table)
                    .drop_column(MegaCl::ClosedAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum MegaCl {
    Table,
    ClosedAt,
}
