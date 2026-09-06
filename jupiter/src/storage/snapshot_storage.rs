//! Persistent source identity and immutable scope attestations. Registry names
//! and live refs are intentionally not foreign keys: historical proofs must
//! survive their cleanup. Publishers can use the same database transaction.

use callisto::{snapshot_instance, snapshot_source, source_commit_scope};
use common::errors::MegaError;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set, sea_query::OnConflict,
};
use sha2::{Digest, Sha256};

use super::base_storage::{BaseStorage, StorageConnector};

const INSTANCE_ROW: &str = "default";

#[derive(Clone)]
pub struct SnapshotStorage {
    pub base: BaseStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Native,
    Import,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Import => "import",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeProofKind {
    ImportCommit,
    NativeRoot,
    NativeScopeProjection,
    NativeReceivePack,
    NativeMerge,
}

impl ScopeProofKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ImportCommit => "import_commit",
            Self::NativeRoot => "native_root",
            Self::NativeScopeProjection => "native_scope_projection",
            Self::NativeReceivePack => "native_receive_pack",
            Self::NativeMerge => "native_merge",
        }
    }
}

/// The application must verify the actual object/scope relationship before
/// writing this record. Syntactic validation here is defense in depth only.
#[derive(Debug, Clone)]
pub struct ScopeAttestation {
    pub source_id: String,
    pub scope_path: String,
    pub commit_oid: String,
    pub root_tree_oid: String,
    pub proof_kind: ScopeProofKind,
    pub proof_oid: Option<String>,
}

impl SnapshotStorage {
    pub async fn ensure_source(
        &self,
        kind: SourceKind,
        repo_id: i64,
    ) -> Result<snapshot_source::Model, MegaError> {
        self.ensure_source_in(self.base.get_connection(), kind, repo_id)
            .await
    }

    /// Stable under retries, concurrent registration and process restarts.
    /// Caller resolves/authorizes repo_id separately; native uses reserved ID 0.
    pub async fn ensure_source_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        kind: SourceKind,
        repo_id: i64,
    ) -> Result<snapshot_source::Model, MegaError> {
        if (kind == SourceKind::Native && repo_id != 0)
            || (kind == SourceKind::Import && repo_id <= 0)
        {
            return Err(MegaError::bad_request("invalid snapshot backend identity"));
        }
        let now = chrono::Utc::now();
        let inserted = snapshot_instance::Entity::insert(snapshot_instance::ActiveModel {
            singleton: Set(INSTANCE_ROW.into()),
            instance_id: Set(uuid::Uuid::new_v4().to_string()),
            created_at: Set(now),
        })
        .on_conflict(
            OnConflict::column(snapshot_instance::Column::Singleton)
                .do_nothing()
                .to_owned(),
        )
        .exec(conn)
        .await;
        ignore_existing(inserted)?;
        let instance = snapshot_instance::Entity::find_by_id(INSTANCE_ROW.to_owned())
            .one(conn)
            .await?
            .ok_or_else(|| MegaError::Unavailable("snapshot instance disappeared".into()))?;
        let inserted = snapshot_source::Entity::insert(snapshot_source::ActiveModel {
            source_id: Set(uuid::Uuid::new_v4().to_string()),
            instance_id: Set(instance.instance_id.clone()),
            kind: Set(kind.as_str().into()),
            repo_id: Set(repo_id),
            created_at: Set(now),
        })
        .on_conflict(
            OnConflict::columns([
                snapshot_source::Column::InstanceId,
                snapshot_source::Column::Kind,
                snapshot_source::Column::RepoId,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec(conn)
        .await;
        ignore_existing(inserted)?;
        snapshot_source::Entity::find()
            .filter(snapshot_source::Column::InstanceId.eq(instance.instance_id))
            .filter(snapshot_source::Column::Kind.eq(kind.as_str()))
            .filter(snapshot_source::Column::RepoId.eq(repo_id))
            .one(conn)
            .await?
            .ok_or_else(|| MegaError::Unavailable("snapshot source disappeared".into()))
    }

    pub async fn source(
        &self,
        source_id: &str,
    ) -> Result<Option<snapshot_source::Model>, MegaError> {
        Ok(snapshot_source::Entity::find_by_id(source_id.to_owned())
            .one(self.base.get_connection())
            .await?)
    }

    /// Insert once; same key + different root or path is never an upsert.
    /// This accepts DatabaseTransaction so proof, objects and refs can commit together.
    pub async fn record_scope_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        proof: &ScopeAttestation,
    ) -> Result<(), MegaError> {
        validate_attestation(proof)?;
        let key = scope_key(&proof.scope_path);
        let inserted = source_commit_scope::Entity::insert(source_commit_scope::ActiveModel {
            source_id: Set(proof.source_id.clone()),
            scope_key: Set(key.clone()),
            scope_path: Set(proof.scope_path.clone()),
            object_format: Set("sha1".into()),
            commit_oid: Set(proof.commit_oid.clone()),
            root_tree_oid: Set(proof.root_tree_oid.clone()),
            proof_kind: Set(proof.proof_kind.as_str().into()),
            proof_oid: Set(proof.proof_oid.clone()),
            created_at: Set(chrono::Utc::now()),
        })
        .on_conflict(
            OnConflict::columns([
                source_commit_scope::Column::SourceId,
                source_commit_scope::Column::ScopeKey,
                source_commit_scope::Column::ObjectFormat,
                source_commit_scope::Column::CommitOid,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec(conn)
        .await;
        ignore_existing(inserted)?;
        let saved = source_commit_scope::Entity::find_by_id((
            proof.source_id.clone(),
            key,
            "sha1".to_owned(),
            proof.commit_oid.clone(),
        ))
        .one(conn)
        .await?
        .ok_or_else(|| MegaError::Unavailable("scope proof disappeared".into()))?;
        if saved.scope_path != proof.scope_path || saved.root_tree_oid != proof.root_tree_oid {
            return Err(MegaError::Conflict(
                "immutable source scope proof mismatch".into(),
            ));
        }
        Ok(())
    }

    pub async fn scope(
        &self,
        source_id: &str,
        path: &str,
        commit_oid: &str,
    ) -> Result<Option<source_commit_scope::Model>, MegaError> {
        let result = source_commit_scope::Entity::find_by_id((
            source_id.to_owned(),
            scope_key(path),
            "sha1".to_owned(),
            commit_oid.to_owned(),
        ))
        .one(self.base.get_connection())
        .await?;
        if result
            .as_ref()
            .is_some_and(|proof| proof.scope_path != path)
        {
            return Err(MegaError::Unavailable(
                "scope path digest collision or corrupt proof".into(),
            ));
        }
        Ok(result)
    }
}

fn ignore_existing<T>(result: Result<T, DbErr>) -> Result<(), MegaError> {
    match result {
        Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn scope_key(path: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mega.scope-path.v1\0");
    hash.update(path.as_bytes());
    hex::encode(hash.finalize())
}

fn valid_oid(oid: &str) -> bool {
    oid.len() == 40
        && oid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_attestation(proof: &ScopeAttestation) -> Result<(), MegaError> {
    let valid_path = proof.scope_path == "/"
        || (proof.scope_path.len() <= 4096
            && proof.scope_path.strip_prefix('/').is_some_and(|relative| {
                relative.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && !part.contains('\0')
                        && part.len() <= 255
                })
            }));
    let valid_source = uuid::Uuid::parse_str(&proof.source_id)
        .is_ok_and(|id| !id.is_nil() && id.to_string() == proof.source_id);
    if !valid_path
        || !valid_source
        || !valid_oid(&proof.commit_oid)
        || !valid_oid(&proof.root_tree_oid)
        || proof.proof_oid.as_ref().is_some_and(|oid| !valid_oid(oid))
    {
        return Err(MegaError::bad_request("invalid source scope attestation"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
