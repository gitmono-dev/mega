use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CampsiteMemberIdentity::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CampsiteMemberIdentity::CampsiteUserId)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CampsiteMemberIdentity::Username)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(string_null(CampsiteMemberIdentity::GithubLogin))
                    .col(
                        ColumnDef::new(CampsiteMemberIdentity::DisplayName)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(CampsiteMemberIdentity::Email)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(date_time(CampsiteMemberIdentity::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_campsite_member_identity_username")
                    .table(CampsiteMemberIdentity::Table)
                    .col(CampsiteMemberIdentity::Username)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_campsite_member_identity_github_login")
                    .table(CampsiteMemberIdentity::Table)
                    .col(CampsiteMemberIdentity::GithubLogin)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_campsite_member_identity_github_login")
                    .table(CampsiteMemberIdentity::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_campsite_member_identity_username")
                    .table(CampsiteMemberIdentity::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(CampsiteMemberIdentity::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CampsiteMemberIdentity {
    Table,
    CampsiteUserId,
    Username,
    GithubLogin,
    DisplayName,
    Email,
    UpdatedAt,
}
