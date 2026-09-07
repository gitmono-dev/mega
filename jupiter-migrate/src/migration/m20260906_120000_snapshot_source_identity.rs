//! Additive source identity/provenance storage. This does not enable namespace
//! publication or backfill historical scope claims automatically.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SnapshotInstance::Table)
                    .if_not_exists()
                    .col(string(SnapshotInstance::Singleton).primary_key())
                    .col(string(SnapshotInstance::InstanceId).unique_key())
                    .col(timestamp(SnapshotInstance::CreatedAt).default(Expr::current_timestamp()))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(SnapshotSource::Table)
                    .if_not_exists()
                    .col(string(SnapshotSource::SourceId).primary_key())
                    .col(string(SnapshotSource::InstanceId))
                    .col(string(SnapshotSource::Kind))
                    // Native source uses repo_id=0. Imported repo IDs are positive and
                    // never reused; this is not a cascading FK to the mutable registry.
                    .col(big_integer(SnapshotSource::RepoId))
                    .col(timestamp(SnapshotSource::CreatedAt).default(Expr::current_timestamp()))
                    .index(
                        Index::create()
                            .name("uq_snapshot_source_backend")
                            .unique()
                            .col(SnapshotSource::InstanceId)
                            .col(SnapshotSource::Kind)
                            .col(SnapshotSource::RepoId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_snapshot_source_instance")
                            .from(SnapshotSource::Table, SnapshotSource::InstanceId)
                            .to(SnapshotInstance::Table, SnapshotInstance::InstanceId)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(SourceCommitScope::Table)
                    .if_not_exists()
                    .col(string(SourceCommitScope::SourceId))
                    // Index the digest, not up to 4096 UTF-8 path bytes: long path keys
                    // can exceed PostgreSQL's btree index tuple size limit.
                    .col(string(SourceCommitScope::ScopeKey))
                    .col(text(SourceCommitScope::ScopePath))
                    .col(string(SourceCommitScope::ObjectFormat))
                    .col(string(SourceCommitScope::CommitOid))
                    .col(string(SourceCommitScope::RootTreeOid))
                    .col(string(SourceCommitScope::ProofKind))
                    .col(string_null(SourceCommitScope::ProofOid))
                    .col(timestamp(SourceCommitScope::CreatedAt).default(Expr::current_timestamp()))
                    .primary_key(
                        Index::create()
                            .col(SourceCommitScope::SourceId)
                            .col(SourceCommitScope::ScopeKey)
                            .col(SourceCommitScope::ObjectFormat)
                            .col(SourceCommitScope::CommitOid),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_scope_snapshot_source")
                            .from(SourceCommitScope::Table, SourceCommitScope::SourceId)
                            .to(SnapshotSource::Table, SnapshotSource::SourceId)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Destructive schema rollback is only for an explicitly requested
        // migration rollback. Application rollback must retain issued snapshots.
        manager
            .drop_table(Table::drop().table(SourceCommitScope::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SnapshotSource::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SnapshotInstance::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum SnapshotInstance {
    Table,
    Singleton,
    InstanceId,
    CreatedAt,
}
#[derive(DeriveIden)]
enum SnapshotSource {
    Table,
    SourceId,
    InstanceId,
    Kind,
    RepoId,
    CreatedAt,
}
#[derive(DeriveIden)]
enum SourceCommitScope {
    Table,
    SourceId,
    ScopeKey,
    ScopePath,
    ObjectFormat,
    CommitOid,
    RootTreeOid,
    ProofKind,
    ProofOid,
    CreatedAt,
}
