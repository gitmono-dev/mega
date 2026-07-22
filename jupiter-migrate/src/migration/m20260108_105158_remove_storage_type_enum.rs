use sea_orm_migration::{
    prelude::*,
    sea_orm::{DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_raw(Statement::from_string(
                        DatabaseBackend::Postgres,
                        "DROP TYPE IF EXISTS storage_type_enum;".to_owned(),
                    ))
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .get_connection()
                    .execute_raw(Statement::from_string(
                        DatabaseBackend::Postgres,
                        r#"
                            CREATE TYPE storage_type_enum AS ENUM (
                                'database',
                                'local_fs',
                                'aws_s3'
                            );
                            "#
                        .to_owned(),
                    ))
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }
}
