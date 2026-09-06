//! Explicit opt-in PostgreSQL gate. Always creates a fresh, randomly named test
//! schema, never refreshes or drops a supplied database. Use a disposable server.

use std::sync::Arc;

use jupiter::storage::{
    base_storage::{BaseStorage, StorageConnector},
    snapshot_storage::{ScopeAttestation, ScopeProofKind, SnapshotStorage, SourceKind},
};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, TransactionTrait};

use super::*;

#[tokio::test]
#[ignore = "requires explicit MEGA_SNAPSHOT_TEST_DATABASE_URL pointing to a disposable loopback PostgreSQL test database"]
async fn postgres_snapshot_nodes_scope_proofs_and_radix_transactions() {
    let url = std::env::var("MEGA_SNAPSHOT_TEST_DATABASE_URL")
        .expect("set explicit disposable PostgreSQL test URL");
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert!(matches!(parsed.scheme(), "postgres" | "postgresql"));
    assert!(matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ));
    assert_eq!(
        parsed.path(),
        "/snapshot_test",
        "use the explicit disposable test database"
    );
    let schema = format!("snapshot_test_{}", uuid::Uuid::new_v4().simple());
    let control = Database::connect(
        ConnectOptions::new(url.clone())
            .max_connections(1)
            .sqlx_logging(false)
            .to_owned(),
    )
    .await
    .unwrap();
    // The interpolated identifier is entirely generated above (ASCII hex).
    control
        .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    let options = ConnectOptions::new(url)
        .max_connections(16)
        .sqlx_logging(false)
        .set_schema_search_path(schema.clone())
        .to_owned();
    let connection = Database::connect(options.clone()).await.unwrap();
    jupiter_migrate::apply_migrations(&connection, false)
        .await
        .unwrap();
    let base = BaseStorage::new(Arc::new(connection));
    let nodes = NamespaceStorage { base: base.clone() };
    let sources = SnapshotStorage { base };
    let conn = nodes.base.get_connection();

    let registered =
        futures::future::join_all((0..16).map(|_| sources.ensure_source(SourceKind::Import, 42)))
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
    assert!(
        registered
            .iter()
            .all(|s| s.source_id == registered[0].source_id)
    );
    let proof = ScopeAttestation {
        source_id: registered[0].source_id.clone(),
        scope_path: format!("/{}", vec!["a".repeat(250); 15].join("/")),
        commit_oid: "1".repeat(40),
        root_tree_oid: "2".repeat(40),
        proof_kind: ScopeProofKind::ImportCommit,
        proof_oid: None,
    };
    let txn = conn.begin().await.unwrap();
    sources.record_scope_in(&txn, &proof).await.unwrap();
    let adapter = DatabaseNodeStore::new(&nodes, &txn);
    let index = RadixIndex::new(&adapter);
    let path = RepoPath::new("/third-party/r").unwrap();
    let old = digest(b"binding A");
    let root = index
        .update(&empty_root(), &path, Some(old.clone()))
        .await
        .unwrap();
    txn.rollback().await.unwrap();
    assert!(nodes.node(root.as_str()).await.unwrap().is_none());
    assert!(
        sources
            .scope(&proof.source_id, &proof.scope_path, &proof.commit_oid)
            .await
            .unwrap()
            .is_none()
    );

    let txn = conn.begin().await.unwrap();
    sources.record_scope_in(&txn, &proof).await.unwrap();
    let adapter = DatabaseNodeStore::new(&nodes, &txn);
    let committed = RadixIndex::new(&adapter)
        .update(&empty_root(), &path, Some(old.clone()))
        .await
        .unwrap();
    assert_eq!(committed, root);
    txn.commit().await.unwrap();
    let bad_proof = ScopeAttestation {
        root_tree_oid: "3".repeat(40),
        ..proof.clone()
    };
    assert!(sources.record_scope_in(conn, &bad_proof).await.is_err());
    let reopened = Database::connect(options).await.unwrap();
    let adapter = DatabaseNodeStore::new(&nodes, &reopened);
    let index = RadixIndex::new(&adapter);
    assert_eq!(index.get(&root, &path).await.unwrap(), Some(old.clone()));
    let new_root = index
        .update(&root, &path, Some(digest(b"binding B")))
        .await
        .unwrap();
    assert_eq!(index.get(&root, &path).await.unwrap(), Some(old));
    assert_ne!(root, new_root);
    let bytes = vec![42; MAX_NODE_BYTES];
    let id = digest(&bytes);
    let writes =
        futures::future::join_all((0..16).map(|_| nodes.put_node_in(conn, id.as_str(), &bytes)))
            .await;
    for write in writes {
        write.unwrap();
    }
    assert_eq!(nodes.node(id.as_str()).await.unwrap().unwrap(), bytes);
    let oversized = vec![42; MAX_NODE_BYTES + 1];
    assert!(
        nodes
            .put_node_in(conn, digest(&oversized).as_str(), &oversized)
            .await
            .is_err()
    );
    println!(
        "PostgreSQL snapshot/node migrations, concurrent source/node inserts, 3765-byte scope key, rollback, reopened reads and immutable root A/B passed; test schema retained: {schema}"
    );
}
