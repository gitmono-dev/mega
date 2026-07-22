use sea_orm_migration::{
    prelude::*,
    sea_orm::{DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. create targets table
        manager
            .create_table(
                Table::create()
                    .table(Targets::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Targets::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Targets::TaskId).uuid().not_null())
                    .col(ColumnDef::new(Targets::TargetPath).string().not_null())
                    .col(ColumnDef::new(Targets::State).string().not_null())
                    .col(ColumnDef::new(Targets::StartAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Targets::EndAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Targets::ErrorSummary).text())
                    .col(
                        ColumnDef::new(Targets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Targets::Table, Targets::TaskId)
                            .to(Tasks::Table, Tasks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique constraint to prevent duplicate targets per task
        manager
            .create_index(
                Index::create()
                    .name("uq_targets_task_path")
                    .table(Targets::Table)
                    .col(Targets::TaskId)
                    .col(Targets::TargetPath)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_targets_task_id")
                    .table(Targets::Table)
                    .col(Targets::TaskId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_targets_state")
                    .table(Targets::Table)
                    .col(Targets::State)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_targets_created_at")
                    .table(Targets::Table)
                    .col(Targets::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 2. add target_id to builds (nullable during migration)
        manager
            .alter_table(
                Table::alter()
                    .table(Builds::Table)
                    .add_column(ColumnDef::new(Builds::TargetId).uuid())
                    .to_owned(),
            )
            .await?;

        // 3. enforce not null + fk (skip SQLite: limited alter support)
        match manager.get_database_backend() {
            DatabaseBackend::Sqlite => Ok(()),
            _ => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Builds::Table)
                            .modify_column(ColumnDef::new(Builds::TargetId).uuid().not_null())
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(Builds::Table)
                            .drop_column(Builds::Target)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .create_foreign_key(
                        ForeignKey::create()
                            .name("fk_builds_target_id")
                            .from(Builds::Table, Builds::TargetId)
                            .to(Targets::Table, Targets::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .to_owned(),
                    )
                    .await
            }
        }
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let _ = manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_builds_target_id")
                    .table(Builds::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("uq_targets_task_path")
                    .table(Targets::Table)
                    .to_owned(),
            )
            .await;
        // restore target column, backfill, drop fk and target_id
        manager
            .alter_table(
                Table::alter()
                    .table(Builds::Table)
                    .add_column(ColumnDef::new(Builds::Target).string())
                    .to_owned(),
            )
            .await?;

        // Backfill builds.target from targets before dropping target_id
        if manager.get_database_backend() != DatabaseBackend::Sqlite {
            let backfill_stmt = Statement::from_string(
                manager.get_database_backend(),
                r#"
                UPDATE builds b
                SET target = t.target_path
                FROM targets t
                WHERE b.target_id = t.id
                "#
                .to_string(),
            );
            manager.get_connection().execute_raw(backfill_stmt).await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Builds::Table)
                    .drop_column(Builds::TargetId)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Targets::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Targets {
    Table,
    Id,
    TaskId,
    TargetPath,
    State,
    StartAt,
    EndAt,
    ErrorSummary,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Builds {
    Table,
    Target,
    TargetId,
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Id,
}
