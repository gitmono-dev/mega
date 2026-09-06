//! Publication metadata only. No initial head, feature flag or historical
//! catalog is synthesized. All fields use portable SQLite/PostgreSQL types.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Meta::NamespaceView)
                    .if_not_exists()
                    .col(string(Meta::ViewId).primary_key())
                    .col(string(Meta::InstanceId))
                    .col(var_binary(Meta::CanonicalBytes, 16384))
                    .col(timestamp_with_time_zone(Meta::CreatedAt))
                    .check(Expr::cust("length(canonical_bytes) <= 16384"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Meta::NamespaceHead)
                    .if_not_exists()
                    .col(string(Meta::InstanceId).primary_key())
                    .col(big_integer(Meta::PublicationSeq))
                    .col(string(Meta::ViewId))
                    .col(big_integer(Meta::WriterEpoch))
                    .check(Expr::cust("publication_seq > 0 AND writer_epoch > 0"))
                    .to_owned(),
            )
            .await?;
        manager.create_table(Table::create()
            .table(Meta::NamespacePublication).if_not_exists()
            .col(string(Meta::InstanceId))
            .col(big_integer(Meta::PublicationSeq))
            .col(string(Meta::ViewId))
            .col(ColumnDef::new(Meta::ParentSeq).big_integer().null())
            .col(ColumnDef::new(Meta::ParentViewId).string().null())
            .col(big_integer(Meta::WriterEpoch))
            .col(string(Meta::ActorDomain))
            .col(string(Meta::OperationId))
            .col(string(Meta::Reason))
            .col(timestamp_with_time_zone(Meta::CreatedAt))
            .primary_key(Index::create().col(Meta::InstanceId).col(Meta::PublicationSeq))
            .check(Expr::cust("publication_seq > 0 AND writer_epoch > 0"))
            .check(Expr::cust("(parent_seq IS NULL AND parent_view_id IS NULL) OR (parent_seq IS NOT NULL AND parent_seq > 0 AND parent_view_id IS NOT NULL)"))
            .to_owned()).await?;
        manager.create_table(Table::create()
            .table(Meta::SnapshotOperation).if_not_exists()
            .col(string(Meta::ActorDomain))
            .col(string(Meta::OperationId))
            .col(string(Meta::InstanceId))
            .col(string(Meta::RequestDigest))
            .col(ColumnDef::new(Meta::PublicationSeq).big_integer().null())
            .col(ColumnDef::new(Meta::ViewId).string().null())
            .col(ColumnDef::new(Meta::Outcome).string().null())
            .col(timestamp_with_time_zone(Meta::CreatedAt))
            .primary_key(Index::create().col(Meta::ActorDomain).col(Meta::OperationId))
            .check(Expr::cust("(publication_seq IS NULL AND view_id IS NULL AND outcome IS NULL) OR (publication_seq IS NOT NULL AND publication_seq > 0 AND view_id IS NOT NULL AND outcome IS NOT NULL AND outcome IN ('published', 'no_op'))"))
            .to_owned()).await?;
        manager
            .create_table(
                Table::create()
                    .table(Meta::NamespaceOutbox)
                    .if_not_exists()
                    .col(string(Meta::EventId).primary_key())
                    .col(string(Meta::InstanceId))
                    .col(big_integer(Meta::PublicationSeq))
                    .col(string(Meta::ViewId))
                    .col(boolean(Meta::Delivered).default(false))
                    .col(timestamp_with_time_zone(Meta::CreatedAt))
                    .check(Expr::cust("publication_seq > 0"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("namespace_outbox_pending")
                    .table(Meta::NamespaceOutbox)
                    .col(Meta::Delivered)
                    .col(Meta::CreatedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Disposable-test DDL rollback only; application rollback retains history.
        for table in [
            Meta::NamespaceOutbox,
            Meta::SnapshotOperation,
            Meta::NamespacePublication,
            Meta::NamespaceHead,
            Meta::NamespaceView,
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Meta {
    NamespaceView,
    NamespaceHead,
    NamespacePublication,
    SnapshotOperation,
    NamespaceOutbox,
    ViewId,
    InstanceId,
    CanonicalBytes,
    CreatedAt,
    PublicationSeq,
    WriterEpoch,
    ParentSeq,
    ParentViewId,
    ActorDomain,
    OperationId,
    Reason,
    RequestDigest,
    Outcome,
    EventId,
    Delivered,
}
