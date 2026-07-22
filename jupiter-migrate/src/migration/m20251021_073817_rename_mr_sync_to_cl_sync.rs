use sea_orm_migration::{
    prelude::*,
    sea_orm::{DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if let DatabaseBackend::Postgres = manager.get_database_backend() {
            let rename_stmt = Statement::from_string(
                manager.get_database_backend(),
                r#"ALTER TYPE "check_type_enum" RENAME VALUE 'mr_sync' TO 'cl_sync';"#,
            );
            manager.get_connection().execute_raw(rename_stmt).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if let DatabaseBackend::Postgres = manager.get_database_backend() {
            let rollback_stmt = Statement::from_string(
                manager.get_database_backend(),
                r#"ALTER TYPE "check_type_enum" RENAME VALUE 'cl_sync' TO 'mr_sync';"#,
            );
            manager.get_connection().execute_raw(rollback_stmt).await?;
        }
        Ok(())
    }
}
