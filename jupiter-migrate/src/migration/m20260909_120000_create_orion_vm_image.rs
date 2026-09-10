//! Catalog of Orion scheduler VM images stored in object storage (RustFS).

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(OrionVmImage::Table)
                    .if_not_exists()
                    .col(string(OrionVmImage::Id).primary_key())
                    .col(
                        ColumnDef::new(OrionVmImage::Digest)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(text(OrionVmImage::ObjectKey))
                    .col(text_null(OrionVmImage::InfoObjectKey))
                    .col(string_null(OrionVmImage::ImageName))
                    .col(string_null(OrionVmImage::BuiltAt))
                    .col(string_null(OrionVmImage::Rust))
                    .col(string_null(OrionVmImage::Buck2))
                    .col(string_null(OrionVmImage::Python))
                    .col(string_null(OrionVmImage::Kernel))
                    .col(big_integer_null(OrionVmImage::SizeBytes))
                    .col(string_null(OrionVmImage::Label))
                    .col(date_time(OrionVmImage::CreatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_orion_vm_image_created_at")
                    .table(OrionVmImage::Table)
                    .col(OrionVmImage::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(OrionVmImage::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum OrionVmImage {
    Table,
    Id,
    Digest,
    ObjectKey,
    InfoObjectKey,
    ImageName,
    BuiltAt,
    Rust,
    Buck2,
    Python,
    Kernel,
    SizeBytes,
    Label,
    CreatedAt,
}
