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
                    .table(CommitAuths::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CommitAuths::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CommitAuths::CommitSha).string().not_null())
                    .col(ColumnDef::new(CommitAuths::AuthorEmail).string().not_null())
                    .col(ColumnDef::new(CommitAuths::MatchedUsername).string().null())
                    .col(
                        ColumnDef::new(CommitAuths::IsAnonymous)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(CommitAuths::MatchedAt).timestamp().null())
                    .col(
                        ColumnDef::new(CommitAuths::CreatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // add index on commit_sha for fast lookup
        manager
            .create_index(
                Index::create()
                    .name("idx_commit_auths_commit_sha")
                    .table(CommitAuths::Table)
                    .col(CommitAuths::CommitSha)
                    .to_owned(),
            )
            .await?;

        // set DB-side default for created_at depending on backend
        let backend = manager.get_database_backend();
        match backend {
            DatabaseBackend::Postgres => {
                // set default to now()
                manager
                    .get_connection()
                    .execute_unprepared(
                        r#"ALTER TABLE commit_auths ALTER COLUMN created_at SET DEFAULT now();"#,
                    )
                    .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {}
            _ => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CommitAuths::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CommitAuths {
    Table,
    Id,
    CommitSha,
    AuthorEmail,
    MatchedUsername,
    IsAnonymous,
    MatchedAt,
    CreatedAt,
}
