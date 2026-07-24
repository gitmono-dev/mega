use sea_orm_migration::{prelude::*, sea_orm::Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db_backend = manager.get_database_backend();
        // CLA remains enabled (still reported) but no longer blocks merge.
        manager
            .get_connection()
            .execute_raw(Statement::from_string(
                db_backend,
                r#"UPDATE path_check_configs SET required = false, updated_at = CURRENT_TIMESTAMP WHERE check_type_code = 'cla_sign';"#,
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db_backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute_raw(Statement::from_string(
                db_backend,
                r#"UPDATE path_check_configs SET required = true, updated_at = CURRENT_TIMESTAMP WHERE check_type_code = 'cla_sign';"#,
            ))
            .await?;

        Ok(())
    }
}
