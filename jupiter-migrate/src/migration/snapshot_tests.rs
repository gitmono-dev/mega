use callisto::git_repo;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, EntityTrait, Set,
    Statement,
};

use super::*;

#[tokio::test]
async fn snapshot_migration_roundtrip_preserves_legacy_tables() {
    let db = Database::connect(
        ConnectOptions::new("sqlite::memory:")
            .max_connections(1)
            .to_owned(),
    )
    .await
    .unwrap();
    Migrator::up(&db, None).await.unwrap();
    let now = chrono::Utc::now().naive_utc();
    git_repo::Entity::insert(git_repo::ActiveModel {
        id: Set(99),
        repo_path: Set("/third-party/schema-fixture".into()),
        repo_name: Set("schema-fixture".into()),
        created_at: Set(now),
        updated_at: Set(now),
    })
    .exec(&db)
    .await
    .unwrap();
    for name in [
        "snapshot_instance",
        "snapshot_source",
        "source_commit_scope",
    ] {
        assert!(has_table(&db, name).await);
    }
    // Explicit destructive DDL rollback is exercised only on this private test
    // database. Production application rollback must retain issued identities.
    Migrator::down(&db, Some(1)).await.unwrap();
    assert!(
        git_repo::Entity::find_by_id(99)
            .one(&db)
            .await
            .unwrap()
            .is_some()
    );
    for name in [
        "snapshot_instance",
        "snapshot_source",
        "source_commit_scope",
    ] {
        assert!(!has_table(&db, name).await);
    }
    Migrator::up(&db, None).await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    assert!(
        git_repo::Entity::find_by_id(99)
            .one(&db)
            .await
            .unwrap()
            .is_some()
    );
    for name in [
        "snapshot_instance",
        "snapshot_source",
        "source_commit_scope",
    ] {
        assert!(has_table(&db, name).await);
    }
}

async fn has_table(db: &DatabaseConnection, name: &str) -> bool {
    db.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
        [name.into()],
    ))
    .await
    .unwrap()
    .is_some()
}
