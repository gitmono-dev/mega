use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UserApprovalStatus::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserApprovalStatus::Username)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(UserApprovalStatus::CampsiteUserId)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(UserApprovalStatus::DisplayName)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(UserApprovalStatus::Email)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(UserApprovalStatus::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(string_null(UserApprovalStatus::ReviewedBy))
                    .col(date_time_null(UserApprovalStatus::ReviewedAt))
                    .col(date_time(UserApprovalStatus::CreatedAt))
                    .col(date_time(UserApprovalStatus::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_user_approval_status_status_created_at")
                    .table(UserApprovalStatus::Table)
                    .col(UserApprovalStatus::Status)
                    .col(UserApprovalStatus::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_user_approval_status_status_created_at")
                    .table(UserApprovalStatus::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(UserApprovalStatus::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum UserApprovalStatus {
    Table,
    Username,
    CampsiteUserId,
    DisplayName,
    Email,
    Status,
    ReviewedBy,
    ReviewedAt,
    CreatedAt,
    UpdatedAt,
}
