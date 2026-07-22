use sea_orm_migration::{prelude::*, sea_orm::Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db_backend = manager.get_database_backend();
        manager
            .get_connection()
            .execute_raw(Statement::from_string(
                db_backend,
                r#"
                    INSERT INTO path_check_configs (created_at, updated_at, id, path, check_type_code, enabled, required)
                    SELECT CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, COALESCE(MAX(id), 0) + 1, '/', 'cla_sign', true, true
                    FROM path_check_configs
                    WHERE NOT EXISTS (
                        SELECT 1 FROM path_check_configs WHERE path = '/' AND check_type_code = 'cla_sign'
                    );
                "#,
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
                r#"DELETE FROM path_check_configs WHERE path = '/' AND check_type_code = 'cla_sign';"#,
            ))
            .await?;

        Ok(())
    }
}
