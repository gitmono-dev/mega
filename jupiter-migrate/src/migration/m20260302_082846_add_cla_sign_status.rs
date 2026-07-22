use sea_orm_migration::{
    prelude::*,
    schema::*,
    sea_orm::{DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ClaSignStatus::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ClaSignStatus::Username)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ClaSignStatus::ClaSigned)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(date_time_null(ClaSignStatus::ClaSignedAt))
                    .col(date_time(ClaSignStatus::CreatedAt))
                    .col(date_time(ClaSignStatus::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        if let DatabaseBackend::Postgres = manager.get_database_backend() {
            let add_check_type_stmt = Statement::from_string(
                manager.get_database_backend(),
                r#"ALTER TYPE "check_type_enum" ADD VALUE IF NOT EXISTS 'cla_sign';"#,
            );
            manager
                .get_connection()
                .execute_raw(add_check_type_stmt)
                .await?;
            // Ensure the new enum value 'cla_sign' is added before inserting the config
            manager
                .get_connection()
                .execute_unprepared("COMMIT; BEGIN;")
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ClaSignStatus::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ClaSignStatus {
    Table,
    Username,
    ClaSigned,
    ClaSignedAt,
    CreatedAt,
    UpdatedAt,
}
