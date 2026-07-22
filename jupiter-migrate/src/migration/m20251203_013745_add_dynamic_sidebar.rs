use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DynamicSidebar::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DynamicSidebar::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DynamicSidebar::PublicId).string().not_null())
                    .col(ColumnDef::new(DynamicSidebar::Label).string().not_null())
                    .col(ColumnDef::new(DynamicSidebar::Href).string().not_null())
                    .col(ColumnDef::new(DynamicSidebar::Visible).boolean().not_null())
                    .col(
                        ColumnDef::new(DynamicSidebar::OrderIndex)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx_dynamic_sidebar_public_id_unique")
                    .table(DynamicSidebar::Table)
                    .col(DynamicSidebar::PublicId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx_dynamic_sidebar_order_index")
                    .table(DynamicSidebar::Table)
                    .col(DynamicSidebar::OrderIndex)
                    .to_owned(),
            )
            .await?;

        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                let sql = r#"
                    INSERT INTO dynamic_sidebar (public_id, label, href, visible, order_index) VALUES
                    ('home', 'Home', '/posts', true, 0),
                    ('inbox', 'Inbox', '/inbox', true, 1),
                    ('chat', 'Chat', '/chat', true, 2),
                    ('notes', 'Docs', '/notes', true, 3),
                    ('calls', 'Calls', '/calls', true, 4),
                    ('drafts', 'Drafts', '/drafts', true, 5),
                    ('code', 'Code', '/code', true, 6),
                    ('tags', 'Tags', '/code/tags', true, 7),
                    ('cl', 'Change List', '/cl', true, 8),
                    ('mq', 'Merge Queue', '/queue/main', true, 9),
                    ('issue', 'Issue', '/issue', true, 10),
                    ('rust', 'Rust', '/rust', true, 11);
                "#;

                manager.get_connection().execute_unprepared(sql).await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DynamicSidebar::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum DynamicSidebar {
    Table,
    Id,
    PublicId,
    Label,
    Href,
    Visible,
    OrderIndex,
}
