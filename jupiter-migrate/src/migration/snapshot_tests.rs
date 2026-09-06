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
        "namespace_node",
    ] {
        assert!(has_table(&db, name).await);
    }
    // Explicit destructive DDL rollback is exercised only on this private test
    // database. Production application rollback must retain issued identities.
    Migrator::down(&db, Some(3)).await.unwrap();
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
        "namespace_node",
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
        "namespace_node",
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

#[tokio::test]
#[ignore = "requires MEGA_SNAPSHOT_TEST_DATABASE_URL for a disposable loopback PostgreSQL test database"]
async fn snapshot_postgres_timestamp_forward_migration_preserves_existing_utc_values() {
    let url = std::env::var("MEGA_SNAPSHOT_TEST_DATABASE_URL")
        .expect("set explicit disposable PostgreSQL test URL");
    // This opt-in test only creates a fresh schema. It never drops/refreshes a
    // supplied database or rewrites existing application schemas.
    let parsed = url::Url::parse(&url).unwrap();
    assert!(matches!(parsed.scheme(), "postgres" | "postgresql"));
    assert!(matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ));
    assert_eq!(
        parsed.path(),
        "/snapshot_test",
        "use the disposable snapshot_test database"
    );
    let schema = format!("snapshot_time_test_{}", common::utils::generate_id());
    let control = Database::connect(
        ConnectOptions::new(url.clone())
            .max_connections(1)
            .sqlx_logging(false)
            .to_owned(),
    )
    .await
    .unwrap();
    control
        .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    let db = Database::connect(
        ConnectOptions::new(url)
            .max_connections(1)
            .sqlx_logging(false)
            .set_schema_search_path(schema.clone())
            .to_owned(),
    )
    .await
    .unwrap();
    Migrator::up(&db, Some(Migrator::migrations().len() as u32 - 1))
        .await
        .unwrap();
    db.execute_unprepared("INSERT INTO snapshot_instance (singleton, instance_id, created_at) VALUES ('utc-fixture', '11111111-1111-4111-8111-111111111111', TIMESTAMP '2024-01-02 03:04:05')").await.unwrap();
    db.execute_unprepared("SET TIME ZONE 'America/Los_Angeles'")
        .await
        .unwrap();
    Migrator::up(&db, None).await.unwrap();
    let model = callisto::snapshot_instance::Entity::find_by_id("utc-fixture".to_owned())
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let expected = chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z").unwrap();
    assert_eq!(model.created_at.timestamp(), expected.timestamp());
    Migrator::down(&db, Some(1)).await.unwrap();
    let row = db.query_one_raw(Statement::from_string(DbBackend::Postgres, "SELECT created_at::text AS stamp FROM snapshot_instance WHERE singleton = 'utc-fixture'")).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "stamp").unwrap(),
        "2024-01-02 03:04:05"
    );
    Migrator::up(&db, None).await.unwrap();
    let model = callisto::snapshot_instance::Entity::find_by_id("utc-fixture".to_owned())
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.created_at.timestamp(), expected.timestamp());
    println!(
        "UTC timestamp forward/down/up verified under non-UTC session; retained test schema: {schema}"
    );
}
