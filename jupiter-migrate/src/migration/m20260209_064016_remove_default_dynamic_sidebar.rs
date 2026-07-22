//! Migration to Clean Up Historical Default Data in `dynamic_sidebar` Table
//!
//! This migration exists solely to address a historical technical debt in
//! `jupiter/src/migration/m20251203_013745_add_dynamic_sidebar.rs`.
//! In that original migration, default menu items were hard-coded and inserted
//! into the `dynamic_sidebar` table. This approach mixes schema definition
//! with data initialization, which is no longer the desired practice.
//!
//! Key principles applied in this migration:
//!
//! 1. **Schema vs. Data Separation**
//!    - Migrations should only define or modify database **schema** (tables, columns, indexes).
//!    - They should **not** insert default or seed data.  
//!    - Default menu data is now managed at **runtime** via a bootstrap method (`DynamicSidebarStorage::bootstrap_sidebar`) that reads from configuration (e.g., `sidebar.default.toml`).
//!
//! 2. **Purpose of This Migration**
//!    - Remove historical default data inserted by the old migration.
//!    - Keep the table structure and indexes intact, so the table is empty but ready for runtime seeding.
//!    - This ensures **new environments** or **fresh deployments** do not carry legacy hard-coded menu items.
//!
//! 3. **Future Default Menu Management**
//!    - Default menu items are now fully configurable and maintained via `sidebar.default.toml` or application config.
//!    - `DynamicSidebarStorage::bootstrap_sidebar` is responsible for checking if the table is empty and inserting defaults at runtime.
//!    - This approach provides flexibility, maintains backward compatibility, and separates schema migrations from dynamic, configurable data.

use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                let sql = r#"DELETE FROM dynamic_sidebar;"#;
                manager.get_connection().execute_unprepared(sql).await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
