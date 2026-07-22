use extension::postgres::Type;
use sea_orm_migration::{
    prelude::*,
    schema::*,
    sea_orm::{DatabaseBackend, EnumIter, Iterable},
};

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
                            .as_enum(CheckTypeEnum)
                            .values(CheckType::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        manager
            .create_table(
                table_auto(PathCheckConfigs::Table)
                    .col(pk_bigint(PathCheckConfigs::Id))
                    .col(string(PathCheckConfigs::Path))
                    .col(enumeration(
                        PathCheckConfigs::CheckTypeCode,
                        Alias::new("check_type_enum"),
                        CheckType::iter(),
                    ))
                    .col(boolean(PathCheckConfigs::Enabled))
                    .col(boolean(PathCheckConfigs::Required))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                table_auto(CheckResult::Table)
                    .col(pk_bigint(CheckResult::Id))
                    .col(string(CheckResult::Path))
                    .col(string(CheckResult::MrLink))
                    .col(string(CheckResult::CommitId))
                    .col(enumeration(
                        CheckResult::CheckTypeCode,
                        Alias::new("check_type_enum"),
                        CheckType::iter(),
                    ))
                    .col(string(CheckResult::Status))
                    .col(string(CheckResult::Message))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("check_res_unique")
                    .table(CheckResult::Table)
                    .col(CheckResult::MrLink)
                    .col(CheckResult::CheckTypeCode)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("path_check_type_unique")
                    .table(PathCheckConfigs::Table)
                    .col(PathCheckConfigs::Path)
                    .col(PathCheckConfigs::CheckTypeCode)
                    .unique()
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
enum PathCheckConfigs {
    Table,
    Id,
    Path,
    CheckTypeCode,
    Enabled,
    Required,
}

#[derive(DeriveIden)]
struct CheckTypeEnum;

#[derive(Iden, EnumIter)]
pub enum CheckType {
    GpgSignature,
    BranchProtection,
    CommitMessage,
    MrSync,
    MergeConflict,
    CiStatus,
    CodeReview,
}

#[derive(DeriveIden)]
enum CheckResult {
    Table,
    Id,
    Path,
    MrLink,
    CommitId,
    CheckTypeCode,
    Status,
    Message,
}
