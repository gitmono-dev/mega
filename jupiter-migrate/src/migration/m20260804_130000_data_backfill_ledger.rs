//! Ledger for application-level data backfills (not schema migrations).
//! Used by mono startup identity backfill (`actor_campsite_user_id_v1`).

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DataBackfillLedger::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DataBackfillLedger::Name)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DataBackfillLedger::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(DataBackfillLedger::Error).text().null())
                    .col(
                        timestamp(DataBackfillLedger::CreatedAt).default(Expr::current_timestamp()),
                    )
                    .col(
                        timestamp(DataBackfillLedger::UpdatedAt).default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Seed the known backfill so claim UPDATE can target a row.
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            INSERT INTO data_backfill_ledger (name, status)
            VALUES ('actor_campsite_user_id_v1', 'pending')
            ON CONFLICT (name) DO NOTHING
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DataBackfillLedger::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum DataBackfillLedger {
    Table,
    Name,
    Status,
    Error,
    CreatedAt,
    UpdatedAt,
}
