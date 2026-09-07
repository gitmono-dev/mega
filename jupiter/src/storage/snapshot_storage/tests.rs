use callisto::{import_refs, sea_orm_active_enums::RefTypeEnum};
use tempfile::TempDir;

use super::*;

fn proof(source_id: &str, path: &str) -> ScopeAttestation {
    ScopeAttestation {
        source_id: source_id.into(),
        scope_path: path.into(),
        commit_oid: "1".repeat(40),
        root_tree_oid: "2".repeat(40),
        proof_kind: ScopeProofKind::NativeScopeProjection,
        proof_oid: Some("3".repeat(40)),
    }
}

#[tokio::test]
async fn source_identity_is_persistent_and_backend_scoped() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let native = snapshots
        .ensure_source(SourceKind::Native, 0)
        .await
        .unwrap();
    let imported = snapshots
        .ensure_source(SourceKind::Import, 17)
        .await
        .unwrap();
    let another = snapshots
        .ensure_source(SourceKind::Import, 18)
        .await
        .unwrap();
    assert_eq!(native.instance_id, imported.instance_id);
    assert_ne!(native.source_id, imported.source_id);
    assert_ne!(imported.source_id, another.source_id);
    let connection =
        sea_orm::Database::connect(format!("sqlite://{}", dir.path().join("test.db").display()))
            .await
            .unwrap();
    let reopened = SnapshotStorage {
        base: BaseStorage::new(std::sync::Arc::new(connection)),
    };
    assert_eq!(
        reopened
            .ensure_source(SourceKind::Import, 17)
            .await
            .unwrap()
            .source_id,
        imported.source_id
    );
    assert_eq!(
        reopened
            .source(&native.source_id)
            .await
            .unwrap()
            .unwrap()
            .repo_id,
        0
    );
    assert!(
        snapshots
            .ensure_source(SourceKind::Native, 17)
            .await
            .is_err()
    );
    assert!(
        snapshots
            .ensure_source(SourceKind::Import, 0)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_registration_allocates_one_identity() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let results =
        futures::future::join_all((0..16).map(|_| snapshots.ensure_source(SourceKind::Import, 42)))
            .await;
    let sources = results.into_iter().map(Result::unwrap).collect::<Vec<_>>();
    assert!(
        sources
            .iter()
            .all(|source| source.source_id == sources[0].source_id
                && source.instance_id == sources[0].instance_id)
    );
}

#[tokio::test]
async fn proof_allows_multiple_scopes_but_never_overwrites_a_root() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let source = snapshots
        .ensure_source(SourceKind::Native, 0)
        .await
        .unwrap();
    let a = proof(&source.source_id, "/project/a");
    let b = proof(&source.source_id, "/project/b");
    let conn = snapshots.base.get_connection();
    snapshots.record_scope_in(conn, &a).await.unwrap();
    snapshots.record_scope_in(conn, &a).await.unwrap();
    snapshots.record_scope_in(conn, &b).await.unwrap();
    for path in ["/project/a", "/project/b"] {
        assert_eq!(
            snapshots
                .scope(&source.source_id, path, &a.commit_oid)
                .await
                .unwrap()
                .unwrap()
                .root_tree_oid,
            a.root_tree_oid
        );
    }
    let mut replacement = a.clone();
    replacement.root_tree_oid = "4".repeat(40);
    assert!(matches!(
        snapshots.record_scope_in(conn, &replacement).await,
        Err(MegaError::Conflict(_))
    ));
    assert_eq!(
        snapshots
            .scope(&source.source_id, &a.scope_path, &a.commit_oid)
            .await
            .unwrap()
            .unwrap()
            .root_tree_oid,
        a.root_tree_oid
    );
    assert!(
        snapshots
            .scope(&source.source_id, "/project/ab", &a.commit_oid)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn scope_proof_participates_in_the_callers_transaction() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let source = snapshots
        .ensure_source(SourceKind::Native, 0)
        .await
        .unwrap();
    let attestation = proof(&source.source_id, "/project/a");
    let txn = storage.begin_db_transaction().await.unwrap();
    snapshots.record_scope_in(&txn, &attestation).await.unwrap();
    txn.rollback().await.unwrap();
    assert!(
        snapshots
            .scope(
                &source.source_id,
                &attestation.scope_path,
                &attestation.commit_oid
            )
            .await
            .unwrap()
            .is_none()
    );
    let txn = storage.begin_db_transaction().await.unwrap();
    snapshots.record_scope_in(&txn, &attestation).await.unwrap();
    txn.commit().await.unwrap();
    assert!(
        snapshots
            .scope(
                &source.source_id,
                &attestation.scope_path,
                &attestation.commit_oid
            )
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn ref_cleanup_does_not_remove_history_and_fk_prevents_source_cascade() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let source = snapshots
        .ensure_source(SourceKind::Import, 17)
        .await
        .unwrap();
    let mut attestation = proof(&source.source_id, "/third-party/lib");
    attestation.proof_kind = ScopeProofKind::ImportCommit;
    let now = chrono::Utc::now().naive_utc();
    storage
        .git_db_storage()
        .save_ref(
            17,
            import_refs::Model {
                id: common::utils::generate_id(),
                repo_id: 17,
                ref_name: "refs/heads/old".into(),
                ref_git_id: attestation.commit_oid.clone(),
                ref_type: RefTypeEnum::Branch,
                default_branch: false,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
    snapshots
        .record_scope_in(snapshots.base.get_connection(), &attestation)
        .await
        .unwrap();
    storage
        .git_db_storage()
        .remove_ref(17, "refs/heads/old")
        .await
        .unwrap();
    assert!(
        snapshots
            .scope(
                &source.source_id,
                &attestation.scope_path,
                &attestation.commit_oid
            )
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        snapshot_source::Entity::delete_by_id(source.source_id.clone())
            .exec(snapshots.base.get_connection())
            .await
            .is_err()
    );
    assert!(snapshots.source(&source.source_id).await.unwrap().is_some());
}

#[tokio::test]
async fn long_paths_use_bounded_index_keys_and_bad_paths_are_rejected() {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let snapshots = storage.snapshot_storage();
    let source = snapshots
        .ensure_source(SourceKind::Native, 0)
        .await
        .unwrap();
    let path = format!("/{}", vec!["x".repeat(250); 15].join("/"));
    let attestation = proof(&source.source_id, &path);
    snapshots
        .record_scope_in(snapshots.base.get_connection(), &attestation)
        .await
        .unwrap();
    let saved = snapshots
        .scope(&source.source_id, &path, &attestation.commit_oid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.scope_key.len(), 64);
    assert_eq!(saved.scope_path, path);
    for invalid in ["relative", "/a/../b", "/a/", "/a//b", "/a\0b"] {
        assert!(
            snapshots
                .record_scope_in(
                    snapshots.base.get_connection(),
                    &proof(&source.source_id, invalid)
                )
                .await
                .is_err()
        );
    }
}
