//! Insert-only, content-addressed namespace index/value storage. No publication
//! head is created or enabled by this additive schema migration.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(NamespaceNode::Table)
                    .if_not_exists()
                    .col(string(NamespaceNode::Digest).primary_key())
                    .col(big_integer(NamespaceNode::SchemaVersion))
                    .col(var_binary(NamespaceNode::CanonicalBytes, 16384))
                    .col(
                        timestamp_with_time_zone(NamespaceNode::CreatedAt)
                            .default(Expr::current_timestamp()),
                    )
                    .check(Expr::cust("length(canonical_bytes) <= 16384"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Explicit DDL rollback only. Application rollback must retain history.
        manager
            .drop_table(Table::drop().table(NamespaceNode::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum NamespaceNode {
    Table,
    Digest,
    SchemaVersion,
    CanonicalBytes,
    CreatedAt,
}
