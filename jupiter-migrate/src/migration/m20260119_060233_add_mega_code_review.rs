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
                            .as_enum(DiffSideEnum)
                            .values(DiffSide::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(ThreadStatusEnum)
                            .values(ThreadStatus::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        // Create code review thread table
        manager
            .create_table(
                Table::create()
                    .table(MegaCodeReviewThread::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaCodeReviewThread::Id))
                    .col(string(MegaCodeReviewThread::Link))
                    .col(string(MegaCodeReviewThread::FilePath))
                    .col(integer(MegaCodeReviewThread::LineNumber))
                    .col(enumeration(
                        MegaCodeReviewThread::DiffSide,
                        Alias::new("diff_side_enum"),
                        DiffSide::iter(),
                    ))
                    .col(enumeration(
                        MegaCodeReviewThread::ThreadStatus,
                        Alias::new("thread_status_enum"),
                        ThreadStatus::iter(),
                    ))
                    .col(date_time(MegaCodeReviewThread::CreatedAt))
                    .col(date_time(MegaCodeReviewThread::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_thread_anchor")
                    .table(MegaCodeReviewThread::Table)
                    .col(MegaCodeReviewThread::Link)
                    .col(MegaCodeReviewThread::FilePath)
                    .col(MegaCodeReviewThread::LineNumber)
                    .col(MegaCodeReviewThread::DiffSide)
                    .to_owned(),
            )
            .await?;

        // create code review comment table
        manager
            .create_table(
                Table::create()
                    .table(MegaCodeReviewComment::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaCodeReviewComment::Id))
                    .col(big_integer(MegaCodeReviewComment::ThreadId))
                    .col(big_integer_null(MegaCodeReviewComment::ParentId))
                    .col(string(MegaCodeReviewComment::UserName))
                    .col(text_null(MegaCodeReviewComment::Content))
                    .col(date_time(MegaCodeReviewComment::CreatedAt))
                    .col(date_time(MegaCodeReviewComment::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_comment_thread")
                            .from(
                                MegaCodeReviewComment::Table,
                                MegaCodeReviewComment::ThreadId,
                            )
                            .to(MegaCodeReviewThread::Table, MegaCodeReviewThread::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_comment_parent")
                            .from(
                                MegaCodeReviewComment::Table,
                                MegaCodeReviewComment::ParentId,
                            )
                            .to(MegaCodeReviewComment::Table, MegaCodeReviewComment::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_comment_thread")
                    .table(MegaCodeReviewComment::Table)
                    .col(MegaCodeReviewComment::ThreadId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_comment_parent")
                    .table(MegaCodeReviewComment::Table)
                    .col(MegaCodeReviewComment::ParentId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MegaCodeReviewComment::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(MegaCodeReviewThread::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(DiffSideEnum).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(ThreadStatusEnum).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum MegaCodeReviewThread {
    Table,
    Id,
    Link,
    FilePath,
    LineNumber,
    DiffSide,
    ThreadStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MegaCodeReviewComment {
    Table,
    Id,
    ThreadId,
    ParentId,
    UserName,
    Content,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
struct DiffSideEnum;
#[derive(Iden, EnumIter)]
pub enum DiffSide {
    New,
    Old,
}

#[derive(DeriveIden)]
struct ThreadStatusEnum;
#[derive(Iden, EnumIter)]
pub enum ThreadStatus {
    Resolved,
    Open,
}
