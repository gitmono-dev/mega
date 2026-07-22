use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

use crate::migration::{m20260119_060233_add_mega_code_review::DiffSide, pk_bigint};

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
                            .as_enum(PositionStatusEnum)
                            .values(PositionStatus::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        // Remove code review thread table `DiffSide`, `LineNumber` and `FilePath`
        manager
            .drop_index(
                Index::drop()
                    .name("idx_thread_anchor")
                    .table(MegaCodeReviewThread::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaCodeReviewThread::Table)
                    .drop_column("file_path")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaCodeReviewThread::Table)
                    .drop_column("diff_side")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MegaCodeReviewThread::Table)
                    .drop_column("line_number")
                    .to_owned(),
            )
            .await?;

        // Create code review anchor
        manager
            .create_table(
                Table::create()
                    .table(MegaCodeReviewAnchor::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaCodeReviewAnchor::Id))
                    .col(big_integer(MegaCodeReviewAnchor::ThreadId))
                    .col(string(MegaCodeReviewAnchor::FilePath))
                    .col(enumeration(
                        MegaCodeReviewAnchor::DiffSide,
                        Alias::new("diff_side_enum"),
                        DiffSide::iter(),
                    ))
                    .col(string(MegaCodeReviewAnchor::AnchorCommitSha))
                    .col(integer(MegaCodeReviewAnchor::OriginalLineNumber))
                    .col(string(MegaCodeReviewAnchor::NormalizedContent))
                    .col(string(MegaCodeReviewAnchor::NormalizedHash))
                    .col(string(MegaCodeReviewAnchor::ContextBefore))
                    .col(string(MegaCodeReviewAnchor::ContextBeforeHash))
                    .col(string(MegaCodeReviewAnchor::ContextAfter))
                    .col(string(MegaCodeReviewAnchor::ContextAfterHash))
                    .col(date_time(MegaCodeReviewAnchor::CreatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_anchor_thread")
                            .from(MegaCodeReviewAnchor::Table, MegaCodeReviewAnchor::ThreadId)
                            .to(MegaCodeReviewThread::Table, MegaCodeReviewThread::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_anchor_thread")
                    .table(MegaCodeReviewAnchor::Table)
                    .col(MegaCodeReviewAnchor::ThreadId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Create mega code review position
        manager
            .create_table(
                Table::create()
                    .table(MegaCodeReviewPosition::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaCodeReviewPosition::Id))
                    .col(big_integer(MegaCodeReviewPosition::AnchorId))
                    .col(string(MegaCodeReviewPosition::CommitSha))
                    .col(string(MegaCodeReviewPosition::FilePath))
                    .col(enumeration(
                        MegaCodeReviewPosition::DiffSide,
                        Alias::new("diff_side_enum"),
                        DiffSide::iter(),
                    ))
                    .col(integer(MegaCodeReviewPosition::LineNumber))
                    .col(integer(MegaCodeReviewPosition::Confidence))
                    .col(enumeration(
                        MegaCodeReviewPosition::PositionStatus,
                        Alias::new("position_status_enum"),
                        PositionStatus::iter(),
                    ))
                    .col(date_time(MegaCodeReviewPosition::CreatedAt))
                    .col(date_time(MegaCodeReviewPosition::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_position_anchor")
                            .from(
                                MegaCodeReviewPosition::Table,
                                MegaCodeReviewPosition::AnchorId,
                            )
                            .to(MegaCodeReviewAnchor::Table, MegaCodeReviewAnchor::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_position_anchor")
                    .table(MegaCodeReviewPosition::Table)
                    .col(MegaCodeReviewPosition::AnchorId)
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
enum MegaCodeReviewThread {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum MegaCodeReviewAnchor {
    Table,
    Id,
    ThreadId,
    FilePath,
    DiffSide,
    AnchorCommitSha,
    OriginalLineNumber,
    NormalizedContent,
    NormalizedHash,
    ContextBefore,
    ContextBeforeHash,
    ContextAfter,
    ContextAfterHash,
    CreatedAt,
}

#[derive(DeriveIden)]
enum MegaCodeReviewPosition {
    Table,
    Id,
    AnchorId,
    CommitSha,
    FilePath,
    DiffSide,
    LineNumber,
    Confidence,
    PositionStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
struct PositionStatusEnum;
#[derive(Iden, EnumIter)]
pub enum PositionStatus {
    Exact,
    Shifted,
    PendingReanchor,
    Ambiguous,
    NotFound,
}
