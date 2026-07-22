use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create target build status enum
        let backend = manager.get_database_backend();
        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .create_type(
                        Type::create()
                            .as_enum(OrionTargetStatusEnum)
                            .values(OrionTargetStatus::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        // Create target build status table
        manager
            .create_table(
                Table::create()
                    .table(TargetBuildStatus::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TargetBuildStatus::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TargetBuildStatus::TaskId).uuid().not_null())
                    .col(
                        ColumnDef::new(TargetBuildStatus::TargetPackage)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TargetBuildStatus::TargetName)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TargetBuildStatus::TargetConfiguration)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TargetBuildStatus::Category)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TargetBuildStatus::Identifier)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TargetBuildStatus::Action).text().not_null())
                    .col(enumeration(
                        TargetBuildStatus::Status,
                        Alias::new("orion_target_status_enum"),
                        OrionTargetStatus::iter(),
                    ))
                    .col(
                        ColumnDef::new(TargetBuildStatus::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TargetBuildStatus::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TargetBuildStatus::Table, TargetBuildStatus::TaskId)
                            .to(OrionTasks::Table, OrionTasks::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // create index
        manager
            .create_index(
                Index::create()
                    .name("idx_task_id")
                    .table(TargetBuildStatus::Table)
                    .col(TargetBuildStatus::TaskId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq_target_build_status_identity")
                    .table(TargetBuildStatus::Table)
                    .col(TargetBuildStatus::TaskId)
                    .col(TargetBuildStatus::TargetPackage)
                    .col(TargetBuildStatus::TargetName)
                    .col(TargetBuildStatus::TargetConfiguration)
                    .col(TargetBuildStatus::Category)
                    .col(TargetBuildStatus::Identifier)
                    .col(TargetBuildStatus::Action)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum TargetBuildStatus {
    Table,
    Id,
    TaskId,
    TargetPackage,
    TargetName,
    TargetConfiguration,
    Category,
    Identifier,
    Action,
    Status,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum OrionTasks {
    Table,
    Id,
}

#[derive(DeriveIden)]
struct OrionTargetStatusEnum;
#[derive(Iden, EnumIter)]
pub enum OrionTargetStatus {
    Pending,
    Running,
    Success,
    Failed,
}
