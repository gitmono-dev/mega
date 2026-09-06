//! Immutable namespace content storage. Publication policy/encoding belongs to
//! Ceres; this boundary enforces size, schema, digest and insert-only durability.

use callisto::namespace_node;
use common::errors::MegaError;
use sea_orm::{ConnectionTrait, DbErr, EntityTrait, Set, sea_query::OnConflict};
use sha2::{Digest, Sha256};

use super::base_storage::{BaseStorage, StorageConnector};

pub const MAX_NAMESPACE_NODE_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct NamespaceStorage {
    pub base: BaseStorage,
}

impl NamespaceStorage {
    pub async fn node(&self, digest: &str) -> Result<Option<Vec<u8>>, MegaError> {
        self.node_in(self.base.get_connection(), digest).await
    }

    pub async fn node_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        digest: &str,
    ) -> Result<Option<Vec<u8>>, MegaError> {
        validate_digest(digest)?;
        let Some(node) = namespace_node::Entity::find_by_id(digest.to_owned())
            .one(conn)
            .await?
        else {
            return Ok(None);
        };
        if node.schema_version != 1
            || node.canonical_bytes.len() > MAX_NAMESPACE_NODE_BYTES
            || node_digest(&node.canonical_bytes) != digest
        {
            return Err(MegaError::Unavailable(
                "corrupt or unsupported namespace node".into(),
            ));
        }
        Ok(Some(node.canonical_bytes))
    }

    /// Accept a caller transaction so immutable nodes and publication metadata
    /// can commit together. Failed prepares never mutate an old view's bytes.
    pub async fn put_node_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        digest: &str,
        bytes: &[u8],
    ) -> Result<(), MegaError> {
        validate_digest(digest)?;
        if bytes.len() > MAX_NAMESPACE_NODE_BYTES || node_digest(bytes) != digest {
            return Err(MegaError::bad_request(
                "invalid namespace node size or digest",
            ));
        }
        let result = namespace_node::Entity::insert(namespace_node::ActiveModel {
            digest: Set(digest.into()),
            schema_version: Set(1),
            canonical_bytes: Set(bytes.to_vec()),
            created_at: Set(chrono::Utc::now().fixed_offset()),
        })
        .on_conflict(
            OnConflict::column(namespace_node::Column::Digest)
                .do_nothing()
                .to_owned(),
        )
        .exec(conn)
        .await;
        match result {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(error) => return Err(error.into()),
        }
        let stored = self
            .node_in(conn, digest)
            .await?
            .ok_or_else(|| MegaError::Unavailable("namespace node disappeared".into()))?;
        if stored != bytes {
            return Err(MegaError::Conflict(
                "immutable namespace node mismatch".into(),
            ));
        }
        Ok(())
    }
}

pub fn node_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub(super) fn validate_digest(digest: &str) -> Result<(), MegaError> {
    if !digest.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(MegaError::bad_request("invalid namespace digest"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
