use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

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
                    .create_type(
                        Type::create()
                            .as_enum(QueueStatusEnum)
                            .values(QueueStatus::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(QueueFailureTypeEnum)
                            .values(QueueFailureType::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        manager
            .create_table(
                Table::create()
                    .table(MergeQueue::Table)
                    .if_not_exists()
                    .col(pk_bigint(MergeQueue::Id))
                    .col(string(MergeQueue::ClLink))
                    .col(enumeration(
                        MergeQueue::Status,
                        Alias::new("queue_status_enum"),
                        QueueStatus::iter(),
                    ))
                    .col(big_integer(MergeQueue::Position))
                    .col(integer(MergeQueue::RetryCount))
                    .col(date_time_null(MergeQueue::LastRetryAt))
                    .col(
                        enumeration_null(
                            MergeQueue::FailureType,
                            Alias::new("queue_failure_type_enum"),
                            QueueFailureType::iter(),
                        )
                        .null(),
                    )
                    .col(text_null(MergeQueue::ErrorMessage))
                    .col(date_time(MergeQueue::CreatedAt))
                    .col(date_time(MergeQueue::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_merge_queue_cl_link")
                    .unique()
                    .table(MergeQueue::Table)
                    .col(MergeQueue::ClLink)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_merge_queue_status_position")
                    .table(MergeQueue::Table)
                    .col(MergeQueue::Status)
                    .col(MergeQueue::Position)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_merge_queue_created_at")
                    .table(MergeQueue::Table)
                    .col(MergeQueue::CreatedAt)
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
enum MergeQueue {
    Table,
    Id,
    ClLink,
    Status,
    Position,
    RetryCount,
    LastRetryAt,
    FailureType,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
struct QueueStatusEnum;

#[derive(Iden, EnumIter)]
pub enum QueueStatus {
    Waiting,
    Testing,
    Merging,
    Merged,
    Failed,
}

#[derive(DeriveIden)]
struct QueueFailureTypeEnum;

#[derive(Iden, EnumIter)]
pub enum QueueFailureType {
    Conflict,
    TestFailure,
    BuildFailure,
    MergeFailure,
    SystemError,
    Timeout,
}
