use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

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
                            .as_enum(ReferenceTypeEnum)
                            .values(ReferenceType::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"ALTER TYPE conv_type_enum ADD VALUE IF NOT EXISTS 'mention';"#,
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        manager
            .create_table(
                table_auto(IssueMrReferences::Table)
                    .if_not_exists()
                    .col(string(IssueMrReferences::SourceId))
                    .col(string(IssueMrReferences::TargetId))
                    .col(enumeration(
                        IssueMrReferences::ReferenceType,
                        Alias::new("reference_type_enum"),
                        ReferenceType::iter(),
                    ))
                    .primary_key(
                        Index::create()
                            .col(IssueMrReferences::SourceId)
                            .col(IssueMrReferences::TargetId),
                    )
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
enum IssueMrReferences {
    Table,
    SourceId,
    TargetId,
    ReferenceType,
}

#[derive(DeriveIden)]
struct ReferenceTypeEnum;

#[derive(Iden, EnumIter)]
pub enum ReferenceType {
    Mention,
    BuildRelates,
    Blocks,
}
