//! Source resolution and fixed-tree membership, independent of live HTTP path
//! routing. This is an internal metadata boundary, NOT authorization, retention,
//! or a namespace publisher. A public read service must enforce those gates.

use common::errors::MegaError;
use jupiter::storage::{
    Storage,
    base_storage::{BaseStorage, StorageConnector},
    git_db_storage::GitDbStorage,
    mono_storage::MonoStorage,
    snapshot_storage::{ScopeAttestation, ScopeProofKind, SnapshotStorage, SourceKind},
};

use super::{
    object::{self, EntryKind, FixedEntry, ObjectKind},
    source::resolve_import_commit,
};
use crate::model::snapshot::{
    ObjectFormat, ObjectId, RelativePath, RepoPath, SourceId, SourceSelector, SourceSnapshot,
};

const MAX_TREE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct SourceCatalog {
    proofs: SnapshotStorage,
    mono: MonoStorage,
    imports: GitDbStorage,
}

impl SourceCatalog {
    pub fn new(storage: &Storage) -> Self {
        Self::from_base(storage.mono_storage().base.clone())
    }

    fn from_base(base: BaseStorage) -> Self {
        Self {
            proofs: SnapshotStorage { base: base.clone() },
            mono: MonoStorage { base: base.clone() },
            imports: GitDbStorage { base },
        }
    }

    pub async fn register_native(&self) -> Result<SourceId, MegaError> {
        let source = self.proofs.ensure_source(SourceKind::Native, 0).await?;
        SourceId::new(source.source_id).map_err(invalid_stored)
    }

    /// Initial lookup only. Once resolved, reads use the persisted backend ID,
    /// never this current registry path. Registration alone publishes no view.
    pub async fn register_import(&self, path: &RepoPath) -> Result<SourceId, MegaError> {
        let repo = self
            .imports
            .find_git_repo_exact_match(path.as_str())
            .await?
            .ok_or_else(|| MegaError::NotFound("import repository not found".into()))?;
        let source = self
            .proofs
            .ensure_source(SourceKind::Import, repo.id)
            .await?;
        SourceId::new(source.source_id).map_err(invalid_stored)
    }

    pub async fn resolve(&self, selector: &SourceSelector) -> Result<SourceSnapshot, MegaError> {
        let (source_id, scope) = match selector {
            SourceSelector::SourceCommit {
                source_id,
                scope_path,
                ..
            }
            | SourceSelector::SourceRef {
                source_id,
                scope_path,
                ..
            } => (source_id, scope_path),
        };
        let backend = self.backend(source_id).await?;

        // A recorded proof is authoritative even after ref/registry removal.
        if let SourceSelector::SourceCommit { commit_oid, .. } = selector
            && let Some(proof) = self
                .proofs
                .scope(source_id.as_str(), scope.as_str(), commit_oid.as_str())
                .await?
        {
            let source = descriptor(
                source_id.clone(),
                scope.clone(),
                commit_oid.as_str(),
                &proof.root_tree_oid,
            )?;
            self.tree(&backend, &source.root_tree_oid).await?;
            return Ok(source);
        }

        let (commit, tree, proof_kind) = match backend.kind.as_str() {
            "import" => {
                // A new observation must use the actual current import root.
                // Multi-source atomicity requires the namespace publisher, not
                // this individually reproducible source observation.
                let repo = self
                    .imports
                    .find_git_repo_by_id(backend.repo_id)
                    .await?
                    .ok_or_else(|| {
                        MegaError::NotFound("import registry entry no longer exists".into())
                    })?;
                if repo.repo_path != scope.as_str() {
                    return Err(MegaError::bad_request(
                        "unattested import scope is not its registered root",
                    ));
                }
                let revision = match selector {
                    SourceSelector::SourceCommit { commit_oid, .. } => commit_oid.as_str(),
                    SourceSelector::SourceRef { ref_name, .. } => ref_name.as_str(),
                };
                let resolved =
                    resolve_import_commit(&self.imports, backend.repo_id, Some(revision)).await?;
                (
                    resolved.commit_oid,
                    resolved.root_tree_oid,
                    ScopeProofKind::ImportCommit,
                )
            }
            "native" => {
                let SourceSelector::SourceRef { ref_name, .. } = selector else {
                    return Err(MegaError::bad_request(
                        "SCOPE_UNKNOWN: native commit has no proof for this scope",
                    ));
                };
                // Ref row captures commit + scope together. Do not accept a
                // commit from the global table and guess that it is a root.
                let selected = self
                    .mono
                    .get_ref_at_path(scope.as_str(), ref_name.as_str())
                    .await?
                    .ok_or_else(|| MegaError::NotFound("native scoped ref not found".into()))?;
                let commit = self
                    .mono
                    .get_commit_by_hash(&selected.ref_commit_hash)
                    .await?
                    .ok_or_else(|| MegaError::NotFound("native commit not found".into()))?;
                if selected.ref_tree_hash != commit.tree {
                    return Err(MegaError::Unavailable(
                        "native ref and commit root tree disagree".into(),
                    ));
                }
                (
                    commit.commit_id,
                    commit.tree,
                    ScopeProofKind::NativeRefObserved,
                )
            }
            _ => unreachable!("backend validates kind"),
        };
        let source = descriptor(source_id.clone(), scope.clone(), &commit, &tree)?;
        self.tree(&backend, &source.root_tree_oid).await?;
        self.record(&source, proof_kind, None).await?;
        Ok(source)
    }

    /// Derive a native child from one already-attested immutable root. The
    /// child retains the root commit's provenance, not a separately moving ref.
    pub async fn project_native(
        &self,
        base: &SourceSnapshot,
        scope: &RepoPath,
    ) -> Result<SourceSnapshot, MegaError> {
        let backend = self.validate(base).await?;
        if backend.kind != "native" {
            return Err(MegaError::bad_request(
                "native projection requires a native source",
            ));
        }
        let path = scope
            .relative_to(&base.scope_path)
            .ok_or_else(|| MegaError::bad_request("projection is outside attested scope"))?;
        let entry = self.locate_in(&backend, base, &path).await?;
        if entry.kind != EntryKind::Directory {
            return Err(MegaError::bad_request(
                "projection target is not a directory",
            ));
        }
        self.tree(&backend, &entry.oid).await?;
        let source = SourceSnapshot {
            scope_path: scope.clone(),
            root_tree_oid: entry.oid,
            ..base.clone()
        };
        self.record(
            &source,
            ScopeProofKind::NativeScopeProjection,
            Some(base.commit_oid.to_string()),
        )
        .await?;
        Ok(source)
    }

    /// Verify the descriptor itself before object access; a caller-supplied
    /// valid-looking root OID is not proof. Does not consult current refs.
    async fn validate(
        &self,
        source: &SourceSnapshot,
    ) -> Result<callisto::snapshot_source::Model, MegaError> {
        let backend = self.backend(&source.source_id).await?;
        let proof = self
            .proofs
            .scope(
                source.source_id.as_str(),
                source.scope_path.as_str(),
                source.commit_oid.as_str(),
            )
            .await?
            .ok_or_else(|| {
                MegaError::bad_request("SCOPE_UNKNOWN: source descriptor is not attested")
            })?;
        if proof.root_tree_oid != source.root_tree_oid.as_str() {
            return Err(MegaError::bad_request(
                "source root does not match attestation",
            ));
        }
        Ok(backend)
    }

    pub async fn locate(
        &self,
        source: &SourceSnapshot,
        path: &RelativePath,
    ) -> Result<FixedEntry, MegaError> {
        let backend = self.validate(source).await?;
        self.locate_in(&backend, source, path).await
    }

    /// Prove the exact (source, path, kind, OID) before a physical CAS fetch.
    /// This is object membership only; the read facade must also check current
    /// source/path authorization and an active retention lease on every read.
    pub async fn prove_object(
        &self,
        source: &SourceSnapshot,
        path: &RelativePath,
        kind: ObjectKind,
        oid: &ObjectId,
    ) -> Result<(), MegaError> {
        let entry = self.locate(source, path).await?;
        if entry.kind.object_kind()? != kind || &entry.oid != oid {
            return Err(MegaError::bad_request(
                "object is not at this fixed-source path",
            ));
        }
        Ok(())
    }

    pub async fn read_tree_payload(
        &self,
        source: &SourceSnapshot,
        path: &RelativePath,
        oid: &ObjectId,
    ) -> Result<Vec<u8>, MegaError> {
        let backend = self.validate(source).await?;
        let entry = self.locate_in(&backend, source, path).await?;
        if entry.kind != EntryKind::Directory || &entry.oid != oid {
            return Err(MegaError::bad_request(
                "tree is not at this fixed-source path",
            ));
        }
        self.tree_payload(&backend, oid).await
    }

    async fn locate_in(
        &self,
        backend: &callisto::snapshot_source::Model,
        source: &SourceSnapshot,
        path: &RelativePath,
    ) -> Result<FixedEntry, MegaError> {
        let mut entry = FixedEntry {
            name: String::new(),
            kind: EntryKind::Directory,
            oid: source.root_tree_oid.clone(),
        };
        if path.as_str().is_empty() {
            return Ok(entry);
        }
        for component in path.as_str().split('/') {
            if entry.kind != EntryKind::Directory {
                return Err(MegaError::bad_request(
                    "snapshot traversal target is not a directory",
                ));
            }
            entry = self
                .tree(backend, &entry.oid)
                .await?
                .into_iter()
                .find(|entry| entry.name == component)
                .ok_or_else(|| MegaError::NotFound("path not found in fixed source".into()))?;
        }
        Ok(entry)
    }

    async fn backend(
        &self,
        source: &SourceId,
    ) -> Result<callisto::snapshot_source::Model, MegaError> {
        let backend = self
            .proofs
            .source(source.as_str())
            .await?
            .ok_or_else(|| MegaError::NotFound("snapshot source not found".into()))?;
        if !((backend.kind == "native" && backend.repo_id == 0)
            || (backend.kind == "import" && backend.repo_id > 0))
        {
            return Err(MegaError::Unavailable(
                "invalid stored snapshot backend".into(),
            ));
        }
        Ok(backend)
    }

    async fn tree(
        &self,
        backend: &callisto::snapshot_source::Model,
        oid: &ObjectId,
    ) -> Result<Vec<FixedEntry>, MegaError> {
        object::decode_tree(&self.tree_payload(backend, oid).await?)
    }

    async fn tree_payload(
        &self,
        backend: &callisto::snapshot_source::Model,
        oid: &ObjectId,
    ) -> Result<Vec<u8>, MegaError> {
        let payload = if backend.kind == "native" {
            self.mono
                .get_tree_by_hash(oid.as_str())
                .await?
                .map(|tree| tree.sub_trees)
        } else {
            self.imports
                .get_tree_by_hash(backend.repo_id, oid.as_str())
                .await?
                .map(|tree| tree.sub_trees)
        }
        .ok_or_else(|| MegaError::NotFound("fixed-source tree is missing".into()))?;
        // DB-backed trees are already materialized by the driver. This bound
        // protects decoding, not the database allocation. Blob streaming is a
        // separate read boundary and must enforce its bound before collecting.
        if payload.len() > MAX_TREE_BYTES {
            return Err(MegaError::bad_request("snapshot tree exceeds byte limit"));
        }
        object::verify_object(ObjectKind::Tree, oid, &payload)?;
        Ok(payload)
    }

    async fn record(
        &self,
        source: &SourceSnapshot,
        kind: ScopeProofKind,
        proof_oid: Option<String>,
    ) -> Result<(), MegaError> {
        self.proofs
            .record_scope_in(
                self.proofs.base.get_connection(),
                &ScopeAttestation {
                    source_id: source.source_id.to_string(),
                    scope_path: source.scope_path.to_string(),
                    commit_oid: source.commit_oid.to_string(),
                    root_tree_oid: source.root_tree_oid.to_string(),
                    proof_kind: kind,
                    proof_oid,
                },
            )
            .await
    }
}

fn invalid_stored(error: impl std::fmt::Display) -> MegaError {
    MegaError::Unavailable(format!("invalid stored source identity: {error}"))
}

fn descriptor(
    source_id: SourceId,
    scope_path: RepoPath,
    commit: &str,
    tree: &str,
) -> Result<SourceSnapshot, MegaError> {
    Ok(SourceSnapshot {
        source_id,
        scope_path,
        object_format: ObjectFormat::Sha1,
        commit_oid: ObjectId::new(commit).map_err(invalid_stored)?,
        root_tree_oid: ObjectId::new(tree).map_err(invalid_stored)?,
    })
}

#[cfg(test)]
mod tests;
