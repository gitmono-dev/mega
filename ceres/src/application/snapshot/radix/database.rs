//! NodeStore adapter that borrows the caller's connection or transaction.

use jupiter::{sea_orm::ConnectionTrait, storage::namespace_storage::NamespaceStorage};

use super::*;

pub struct DatabaseNodeStore<'a, C: ConnectionTrait> {
    storage: &'a NamespaceStorage,
    connection: &'a C,
}

impl<'a, C: ConnectionTrait> DatabaseNodeStore<'a, C> {
    pub fn new(storage: &'a NamespaceStorage, connection: &'a C) -> Self {
        Self {
            storage,
            connection,
        }
    }
}

#[async_trait]
impl<C: ConnectionTrait> NodeStore for DatabaseNodeStore<'_, C> {
    async fn read(&self, id: &ManifestDigest) -> Result<Vec<u8>, MegaError> {
        self.storage
            .node_in(self.connection, id.as_str())
            .await?
            .ok_or_else(|| MegaError::Unavailable("namespace index node unavailable".into()))
    }

    async fn write(&self, id: &ManifestDigest, bytes: &[u8]) -> Result<(), MegaError> {
        self.storage
            .put_node_in(self.connection, id.as_str(), bytes)
            .await
    }
}

#[cfg(test)]
mod postgres_tests;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jupiter::storage::base_storage::{BaseStorage, StorageConnector};
    use sea_orm::TransactionTrait;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn radix_database_nodes_follow_the_outer_transaction_and_reopen() {
        let dir = TempDir::new().unwrap();
        let conn = jupiter::tests::test_db_connection(dir.path()).await;
        jupiter_migrate::apply_migrations(&conn, true)
            .await
            .unwrap();
        let storage = NamespaceStorage {
            base: BaseStorage::new(Arc::new(conn)),
        };
        let path = RepoPath::new("/third-party/r").unwrap();
        let old = digest(b"old binding");
        let conn = storage.base.get_connection();
        let txn = conn.begin().await.unwrap();
        let db = DatabaseNodeStore::new(&storage, &txn);
        let index = RadixIndex::new(&db);
        let root = index
            .update(&empty_root(), &path, Some(old.clone()))
            .await
            .unwrap();
        assert_eq!(index.get(&root, &path).await.unwrap(), Some(old.clone()));
        txn.rollback().await.unwrap();
        assert!(storage.node(root.as_str()).await.unwrap().is_none());
        let txn = conn.begin().await.unwrap();
        let db = DatabaseNodeStore::new(&storage, &txn);
        let committed = RadixIndex::new(&db)
            .update(&empty_root(), &path, Some(old.clone()))
            .await
            .unwrap();
        assert_eq!(committed, root);
        txn.commit().await.unwrap();
        let reopened = sea_orm::Database::connect(format!(
            "sqlite://{}",
            dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        let db = DatabaseNodeStore::new(&storage, &reopened);
        assert_eq!(
            RadixIndex::new(&db).get(&root, &path).await.unwrap(),
            Some(old)
        );
    }
}
