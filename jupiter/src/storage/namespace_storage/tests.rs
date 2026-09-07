use callisto::namespace_node;
use sea_orm::{ColumnTrait, QueryFilter, TransactionTrait, sea_query::Expr};
use tempfile::TempDir;

use super::*;

#[tokio::test]
async fn namespace_nodes_are_idempotent_bounded_and_transactional() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let nodes = storage.namespace_storage();
    let conn = nodes.base.get_connection();
    let bytes = b"canonical fixture";
    let digest = node_digest(bytes);
    let txn = conn.begin().await.unwrap();
    nodes.put_node_in(&txn, &digest, bytes).await.unwrap();
    assert_eq!(nodes.node_in(&txn, &digest).await.unwrap().unwrap(), bytes);
    txn.rollback().await.unwrap();
    assert!(nodes.node(&digest).await.unwrap().is_none());
    let txn = conn.begin().await.unwrap();
    nodes.put_node_in(&txn, &digest, bytes).await.unwrap();
    nodes.put_node_in(&txn, &digest, bytes).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(nodes.node(&digest).await.unwrap().unwrap(), bytes);
    assert!(
        nodes
            .put_node_in(conn, &digest, b"different")
            .await
            .is_err()
    );
    let max = vec![42; MAX_NAMESPACE_NODE_BYTES];
    nodes
        .put_node_in(conn, &node_digest(&max), &max)
        .await
        .unwrap();
    let too_large = vec![42; MAX_NAMESPACE_NODE_BYTES + 1];
    assert!(
        nodes
            .put_node_in(conn, &node_digest(&too_large), &too_large)
            .await
            .is_err()
    );
    // The database constraint also protects callers that bypass the facade.
    assert!(
        namespace_node::Entity::insert(namespace_node::ActiveModel {
            digest: Set(node_digest(&too_large)),
            schema_version: Set(1),
            canonical_bytes: Set(too_large),
            created_at: Set(chrono::Utc::now().fixed_offset()),
        })
        .exec(conn)
        .await
        .is_err()
    );
    assert!(nodes.node("sha1:bad").await.is_err());
}

#[tokio::test]
async fn namespace_nodes_reject_corruption_and_unknown_schema() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let nodes = storage.namespace_storage();
    let conn = nodes.base.get_connection();
    let bytes = b"canonical fixture";
    let digest = node_digest(bytes);
    nodes.put_node_in(conn, &digest, bytes).await.unwrap();
    namespace_node::Entity::update_many()
        .col_expr(
            namespace_node::Column::CanonicalBytes,
            Expr::value(b"corrupt".to_vec()),
        )
        .filter(namespace_node::Column::Digest.eq(&digest))
        .exec(conn)
        .await
        .unwrap();
    assert!(matches!(
        nodes.node(&digest).await,
        Err(MegaError::Unavailable(_))
    ));
    assert!(nodes.put_node_in(conn, &digest, bytes).await.is_err());
    namespace_node::Entity::update_many()
        .col_expr(
            namespace_node::Column::CanonicalBytes,
            Expr::value(bytes.to_vec()),
        )
        .col_expr(namespace_node::Column::SchemaVersion, Expr::value(2i64))
        .filter(namespace_node::Column::Digest.eq(&digest))
        .exec(conn)
        .await
        .unwrap();
    assert!(nodes.node(&digest).await.is_err());
}

#[tokio::test]
async fn concurrent_identical_namespace_inserts_return_the_same_bytes() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let nodes = storage.namespace_storage();
    let bytes = b"shared";
    let digest = node_digest(bytes);
    let results = futures::future::join_all(
        (0..16).map(|_| nodes.put_node_in(nodes.base.get_connection(), &digest, bytes)),
    )
    .await;
    for result in results {
        result.unwrap();
    }
    assert_eq!(nodes.node(&digest).await.unwrap().unwrap(), bytes);
}
