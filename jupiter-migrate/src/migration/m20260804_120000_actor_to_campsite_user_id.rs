//! Rename/replace actor string columns with `campsite_user_id`, and add
//! `access_token.github_login` / `mega_cl_reviewer.github_login` for collaboration identity.
//!
//! Data backfill (handle → campsite_user_id) runs automatically on mono startup
//! via `data_backfill_ledger` + Campsite `internal/member_identities`
//! (`oauth.mega_internal_secret` / `MEGA_INTERNAL_SECRET`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        match manager.get_database_backend() {
            DatabaseBackend::Postgres => up_postgres(manager).await,
            DatabaseBackend::Sqlite => up_sqlite(manager).await,
            other => Err(DbErr::Custom(format!(
                "unsupported database backend for actor→campsite_user_id migration: {other:?}"
            ))),
        }
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible without restoring dropped username data.
        Ok(())
    }
}

async fn up_postgres(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // --- access_token: add github_login (before username → campsite_user_id rename) ---
    db.execute_unprepared(
        r#"ALTER TABLE access_token ADD COLUMN IF NOT EXISTS github_login VARCHAR"#,
    )
    .await?;

    // --- mega_cl: username → campsite_user_id ---
    db.execute_unprepared(r#"ALTER TABLE mega_cl RENAME COLUMN username TO campsite_user_id"#)
        .await?;

    // --- mega_issue: author → campsite_user_id ---
    db.execute_unprepared(r#"ALTER TABLE mega_issue RENAME COLUMN author TO campsite_user_id"#)
        .await?;

    // --- mega_conversation: username → campsite_user_id ---
    db.execute_unprepared(
        r#"ALTER TABLE mega_conversation RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- mega_cl_reviewer: username → campsite_user_id + github_login ---
    db.execute_unprepared(
        r#"ALTER TABLE mega_cl_reviewer ADD COLUMN IF NOT EXISTS github_login VARCHAR"#,
    )
    .await?;
    // Preserve prior handle as github_login for display/Cedar until backfill maps id
    db.execute_unprepared(
        r#"UPDATE mega_cl_reviewer SET github_login = username WHERE github_login IS NULL"#,
    )
    .await?;
    db.execute_unprepared(
        r#"ALTER TABLE mega_cl_reviewer RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- item_assignees: assignnee_id → campsite_user_id ---
    db.execute_unprepared(
        r#"ALTER TABLE item_assignees RENAME COLUMN assignnee_id TO campsite_user_id"#,
    )
    .await?;

    // --- reactions ---
    db.execute_unprepared(r#"ALTER TABLE reactions RENAME COLUMN username TO campsite_user_id"#)
        .await?;

    // --- mega_code_review_comment ---
    db.execute_unprepared(
        r#"ALTER TABLE mega_code_review_comment RENAME COLUMN user_name TO campsite_user_id"#,
    )
    .await?;

    // --- access_token ---
    db.execute_unprepared(r#"ALTER TABLE access_token RENAME COLUMN username TO campsite_user_id"#)
        .await?;

    // --- ssh_keys ---
    db.execute_unprepared(r#"ALTER TABLE ssh_keys RENAME COLUMN username TO campsite_user_id"#)
        .await?;

    // --- cla_sign_status (PK was username) ---
    db.execute_unprepared(
        r#"ALTER TABLE cla_sign_status RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- user_notification_settings ---
    db.execute_unprepared(
        r#"ALTER TABLE user_notification_settings RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- user_notification_preferences ---
    db.execute_unprepared(
        r#"ALTER TABLE user_notification_preferences RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- email_jobs ---
    db.execute_unprepared(r#"ALTER TABLE email_jobs RENAME COLUMN username TO campsite_user_id"#)
        .await?;

    // --- mega_group_member ---
    db.execute_unprepared(
        r#"ALTER TABLE mega_group_member RENAME COLUMN username TO campsite_user_id"#,
    )
    .await?;

    // --- user_approval_status: drop username PK, use campsite_user_id as PK ---
    // reviewed_by stays as opaque actor id string (will hold campsite_user_id going forward)
    db.execute_unprepared(
        r#"
            ALTER TABLE user_approval_status DROP CONSTRAINT IF EXISTS user_approval_status_pkey;
            ALTER TABLE user_approval_status DROP COLUMN IF EXISTS username;
            ALTER TABLE user_approval_status ADD PRIMARY KEY (campsite_user_id);
            "#,
    )
    .await?;

    Ok(())
}

async fn up_sqlite(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // SQLite does not support `ADD COLUMN IF NOT EXISTS`; sea-orm handles it.
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new("access_token"))
                .add_column_if_not_exists(
                    ColumnDef::new(Alias::new("github_login")).string().null(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(Alias::new("mega_cl_reviewer"))
                .add_column_if_not_exists(
                    ColumnDef::new(Alias::new("github_login")).string().null(),
                )
                .to_owned(),
        )
        .await?;

    if sqlite_has_column(db, "mega_cl_reviewer", "username").await? {
        db.execute_unprepared(
            r#"UPDATE mega_cl_reviewer SET github_login = username WHERE github_login IS NULL"#,
        )
        .await?;
    }

    // Plain RENAME COLUMN is supported on modern SQLite.
    // Note: m20250812 skipped user_id→username for access_token/ssh_keys on SQLite.
    for (table, from, to) in [
        ("mega_cl", "username", "campsite_user_id"),
        ("mega_issue", "author", "campsite_user_id"),
        ("mega_conversation", "username", "campsite_user_id"),
        ("mega_cl_reviewer", "username", "campsite_user_id"),
        ("item_assignees", "assignnee_id", "campsite_user_id"),
        ("reactions", "username", "campsite_user_id"),
        ("mega_code_review_comment", "user_name", "campsite_user_id"),
        ("access_token", "username", "campsite_user_id"),
        ("access_token", "user_id", "campsite_user_id"),
        ("ssh_keys", "username", "campsite_user_id"),
        ("ssh_keys", "user_id", "campsite_user_id"),
        ("cla_sign_status", "username", "campsite_user_id"),
        ("user_notification_settings", "username", "campsite_user_id"),
        (
            "user_notification_preferences",
            "username",
            "campsite_user_id",
        ),
        ("email_jobs", "username", "campsite_user_id"),
        ("mega_group_member", "username", "campsite_user_id"),
    ] {
        rename_sqlite_column_if_needed(db, table, from, to).await?;
    }

    // SQLite cannot drop PK / change PK in place — rebuild the table.
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE user_approval_status_new (
            campsite_user_id TEXT NOT NULL PRIMARY KEY,
            display_name TEXT NOT NULL DEFAULT '',
            email TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            reviewed_by TEXT NULL,
            reviewed_at TEXT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        INSERT INTO user_approval_status_new (
            campsite_user_id, display_name, email, status,
            reviewed_by, reviewed_at, created_at, updated_at
        )
        SELECT
            campsite_user_id, display_name, email, status,
            reviewed_by, reviewed_at, created_at, updated_at
        FROM user_approval_status;
        DROP TABLE user_approval_status;
        ALTER TABLE user_approval_status_new RENAME TO user_approval_status;
        CREATE INDEX IF NOT EXISTS idx_user_approval_status_status_created_at
            ON user_approval_status (status, created_at);
        "#
        .to_owned(),
    ))
    .await?;

    Ok(())
}

async fn sqlite_has_column(
    db: &SchemaManagerConnection<'_>,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(r#"PRAGMA table_info("{table}")"#),
        ))
        .await?;

    for row in rows {
        // PRAGMA table_info: cid, name, type, notnull, dflt_value, pk
        if let Ok(name) = row.try_get_by_index::<String>(1)
            && name == column
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn rename_sqlite_column_if_needed(
    db: &SchemaManagerConnection<'_>,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), DbErr> {
    if !sqlite_has_column(db, table, from).await? {
        return Ok(());
    }
    if sqlite_has_column(db, table, to).await? {
        return Ok(());
    }
    db.execute_unprepared(&format!(
        r#"ALTER TABLE "{table}" RENAME COLUMN "{from}" TO "{to}""#
    ))
    .await?;
    Ok(())
}
