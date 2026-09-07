use std::sync::Arc;

use callisto::{git_repo, import_refs, sea_orm_active_enums::RefTypeEnum};
use sea_orm::{ConnectOptions, Database, PaginatorTrait};
use tempfile::TempDir;

use super::*;
use crate::storage::{
    git_db_storage::GitDbStorage,
    snapshot_storage::{SnapshotStorage, SourceKind},
};

struct Fixture {
    publications: PublicationStorage,
    refs: GitDbStorage,
    instance: String,
}

impl Fixture {
    async fn new(base: BaseStorage) -> Self {
        let source = SnapshotStorage { base: base.clone() }
            .ensure_source(SourceKind::Native, 0)
            .await
            .unwrap();
        let conn = base.get_connection();
        let now = chrono::Utc::now().naive_utc();
        git_repo::Entity::insert(git_repo::ActiveModel {
            id: Set(42),
            repo_path: Set("/third-party/publication-fixture".into()),
            repo_name: Set("publication-fixture".into()),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .exec(conn)
        .await
        .unwrap();
        for (id, name) in [(1, "refs/heads/main"), (2, "refs/heads/feature")] {
            import_refs::Entity::insert(import_refs::ActiveModel {
                id: Set(id),
                repo_id: Set(42),
                ref_name: Set(name.into()),
                ref_git_id: Set("a".repeat(40)),
                ref_type: Set(RefTypeEnum::Branch),
                default_branch: Set(id == 1),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .exec(conn)
            .await
            .unwrap();
        }
        Self {
            publications: PublicationStorage { base: base.clone() },
            refs: GitDbStorage { base },
            instance: source.instance_id,
        }
    }

    fn request(&self, label: &str) -> PublicationRequest {
        PublicationRequest {
            instance_id: self.instance.clone(),
            actor_domain: "test:actor".into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            request_digest: node_digest(label.as_bytes()),
        }
    }
    fn view(&self, label: &str) -> PreparedNamespaceView {
        // Opaque storage fixture only. Shared Ceres/ScorpioFS manifest tests use
        // independently framed real descriptors; this facade does not decode them.
        let bytes = format!("{}:view:{label}", self.instance).into_bytes();
        PreparedNamespaceView {
            instance_id: self.instance.clone(),
            view_id: node_digest(&bytes),
            canonical_bytes: bytes,
        }
    }
    async fn ready(
        &self,
        request: PublicationRequest,
        head: Option<PublicationHead>,
    ) -> Box<PublicationTransaction> {
        match self.publications.begin(request, head, 1).await.unwrap() {
            BeginPublication::Ready(txn) => txn,
            BeginPublication::Replay(_) => panic!("unexpected replay"),
        }
    }
    async fn ref_id(&self, id: i64) -> String {
        import_refs::Entity::find_by_id(id)
            .one(self.refs.base.get_connection())
            .await
            .unwrap()
            .unwrap()
            .ref_git_id
    }
    async fn counts(&self) -> (u64, u64, u64) {
        let conn = self.refs.base.get_connection();
        (
            namespace_publication::Entity::find()
                .count(conn)
                .await
                .unwrap(),
            namespace_outbox::Entity::find().count(conn).await.unwrap(),
            snapshot_operation::Entity::find()
                .count(conn)
                .await
                .unwrap(),
        )
    }
}

async fn sqlite() -> (TempDir, Fixture) {
    let dir = TempDir::new().unwrap();
    let storage = crate::tests::test_storage(dir.path()).await;
    let fixture = Fixture::new(storage.mono_storage().base.clone()).await;
    (dir, fixture)
}

async fn lifecycle(f: &Fixture) {
    let request = f.request("bootstrap");
    let initial = f.view("B");
    let txn = f.ready(request.clone(), None).await;
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/main",
                &"a".repeat(40),
                &"b".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    let receipt = txn.finish(&initial, "initial import").await.unwrap();
    assert_eq!(receipt.publication_seq, 1);
    assert_eq!(receipt.outcome, PublicationOutcome::Published);
    assert_eq!(f.ref_id(1).await, "b".repeat(40));
    assert_eq!(f.counts().await, (1, 1, 1));
    assert_eq!(
        f.publications.receipt(&request).await.unwrap(),
        Some(receipt.clone())
    );
    assert!(
        matches!(f.publications.begin(request.clone(), None, 1).await.unwrap(),
        BeginPublication::Replay(found) if found == receipt)
    );
    let changed_request = PublicationRequest {
        request_digest: node_digest(b"different"),
        ..request.clone()
    };
    assert!(matches!(
        f.publications.begin(changed_request, None, 1).await,
        Err(MegaError::Conflict(_))
    ));
    assert_eq!(f.counts().await, (1, 1, 1));

    let head = f.publications.head(&f.instance).await.unwrap();
    let noop_request = f.request("non-selected branch");
    let txn = f.ready(noop_request, head.clone()).await;
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/feature",
                &"a".repeat(40),
                &"c".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    assert_eq!(
        txn.finish(&initial, "non-selected branch")
            .await
            .unwrap()
            .outcome,
        PublicationOutcome::NoOp
    );
    assert_eq!(f.counts().await, (1, 1, 2));
    assert_eq!(f.ref_id(2).await, "c".repeat(40));
    assert_eq!(f.publications.head(&f.instance).await.unwrap(), head);

    // Failure after ref mutation must not commit a receipt, view, head or ref.
    let failed_request = f.request("wrong view instance");
    let txn = f.ready(failed_request.clone(), head.clone()).await;
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/main",
                &"b".repeat(40),
                &"d".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    let candidate = f.view("D");
    let wrong_instance = PreparedNamespaceView {
        instance_id: uuid::Uuid::new_v4().to_string(),
        ..candidate.clone()
    };
    assert!(txn.finish(&wrong_instance, "must rollback").await.is_err());
    assert_eq!(f.ref_id(1).await, "b".repeat(40));
    assert!(
        f.publications
            .receipt(&failed_request)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.publications
            .view(&f.instance, &candidate.view_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.counts().await, (1, 1, 2));

    // Even identical-view publication must execute the writer-epoch fence.
    let fenced_request = f.request("fenced noop");
    let txn = f.ready(fenced_request.clone(), head.clone()).await;
    namespace_head::Entity::update_many()
        .col_expr(namespace_head::Column::WriterEpoch, Expr::value(2i64))
        .filter(namespace_head::Column::InstanceId.eq(&f.instance))
        .exec(txn.transaction())
        .await
        .unwrap();
    assert!(matches!(
        txn.finish(&initial, "fenced noop").await,
        Err(MegaError::Conflict(_))
    ));
    assert!(
        f.publications
            .receipt(&fenced_request)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.publications.head(&f.instance).await.unwrap(), head);

    // Force the publication-row insert to fail AFTER view insert and head CAS.
    // This inserted collision is itself uncommitted and must also roll back.
    let failed_after_cas = f.request("failure after head CAS");
    let txn = f.ready(failed_after_cas.clone(), head.clone()).await;
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/main",
                &"b".repeat(40),
                &"e".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    namespace_publication::Entity::insert(namespace_publication::ActiveModel {
        instance_id: Set(f.instance.clone()),
        publication_seq: Set(2),
        view_id: Set(candidate.view_id.clone()),
        parent_seq: Set(Some(1)),
        parent_view_id: Set(Some(initial.view_id.clone())),
        writer_epoch: Set(1),
        actor_domain: Set(failed_after_cas.actor_domain.clone()),
        operation_id: Set(failed_after_cas.operation_id.clone()),
        reason: Set("injected duplicate publication row".into()),
        created_at: Set(chrono::Utc::now().fixed_offset()),
    })
    .exec(txn.transaction())
    .await
    .unwrap();
    assert!(
        txn.finish(&candidate, "must rollback after CAS")
            .await
            .is_err()
    );
    assert_eq!(f.ref_id(1).await, "b".repeat(40));
    assert_eq!(f.publications.head(&f.instance).await.unwrap(), head);
    assert!(
        f.publications
            .receipt(&failed_after_cas)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.publications
            .view(&f.instance, &candidate.view_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.counts().await, (1, 1, 2));

    // Dropping the owner cannot commit a reserved operation or its ref writes.
    let dropped = f.request("dropped transaction");
    let txn = f.ready(dropped.clone(), head.clone()).await;
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/feature",
                &"c".repeat(40),
                &"f".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    drop(txn);
    // Reserve the same key again: wait on the database lock, not an arbitrary sleep.
    f.ready(dropped.clone(), head.clone())
        .await
        .abort()
        .await
        .unwrap();
    assert_eq!(f.ref_id(2).await, "c".repeat(40));
    assert!(f.publications.receipt(&dropped).await.unwrap().is_none());

    let txn = f.ready(f.request("next root"), head.clone()).await;
    assert!(
        !f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/main",
                &"a".repeat(40),
                &"e".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/main",
                &"b".repeat(40),
                &"d".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    assert_eq!(
        txn.finish(&candidate, "advance")
            .await
            .unwrap()
            .publication_seq,
        2
    );
    assert_eq!(
        f.publications
            .view(&f.instance, &initial.view_id)
            .await
            .unwrap(),
        Some(initial.canonical_bytes)
    );
    assert!(matches!(
        f.publications.begin(f.request("stale head"), head, 1).await,
        Err(MegaError::Conflict(_))
    ));
    assert_eq!(f.counts().await, (2, 2, 3));
}

#[tokio::test]
async fn snapshot_publication_sqlite_ref_receipt_noop_fence_and_rollback() {
    let (_dir, f) = sqlite().await;
    lifecycle(&f).await;
}

async fn duplicate_race(f: &Fixture) {
    let request = f.request("duplicate bootstrap");
    let view = f.view("B");
    let results = futures::future::join_all((0..8).map(|_| async {
        match f
            .publications
            .begin(request.clone(), None, 1)
            .await
            .unwrap()
        {
            BeginPublication::Replay(receipt) => (false, receipt),
            BeginPublication::Ready(txn) => {
                assert!(
                    f.refs
                        .update_ref_if_unchanged(
                            42,
                            "refs/heads/main",
                            &"a".repeat(40),
                            &"b".repeat(40),
                            txn.transaction()
                        )
                        .await
                        .unwrap()
                );
                (true, txn.finish(&view, "duplicate race").await.unwrap())
            }
        }
    }))
    .await;
    assert_eq!(results.iter().filter(|(first, _)| *first).count(), 1);
    assert!(results.iter().all(|(_, r)| r == &results[0].1));
    assert_eq!(f.counts().await, (1, 1, 1));
    assert_eq!(f.ref_id(1).await, "b".repeat(40));
}

#[tokio::test]
async fn snapshot_publication_sqlite_duplicate_operation_is_applied_once() {
    let (_dir, f) = sqlite().await;
    duplicate_race(&f).await;
}

async fn competing_writers(f: &Fixture) {
    let initial = f.view("A");
    f.ready(f.request("bootstrap"), None)
        .await
        .finish(&initial, "initial")
        .await
        .unwrap();
    let expected = f.publications.head(&f.instance).await.unwrap();
    let results = futures::future::join_all(["b", "c"].into_iter().map(|new| {
        let expected = expected.clone();
        async move {
            let request = f.request(new);
            let txn = match f.publications.begin(request.clone(), expected, 1).await {
                Ok(BeginPublication::Ready(txn)) => txn,
                Err(MegaError::Conflict(_)) => return (new, false, request),
                _ => panic!("unexpected begin result"),
            };
            if !f
                .refs
                .update_ref_if_unchanged(
                    42,
                    "refs/heads/main",
                    &"a".repeat(40),
                    &new.repeat(40),
                    txn.transaction(),
                )
                .await
                .unwrap()
            {
                txn.abort().await.unwrap();
                return (new, false, request);
            }
            let success = match txn.finish(&f.view(new), "competing writers").await {
                Ok(_) => true,
                Err(MegaError::Conflict(_)) => false,
                other => panic!("{other:?}"),
            };
            (new, success, request)
        }
    }))
    .await;
    assert_eq!(results.iter().filter(|(_, success, _)| *success).count(), 1);
    for (new, success, request) in results {
        assert_eq!(
            f.publications.receipt(&request).await.unwrap().is_some(),
            success
        );
        if success {
            assert_eq!(f.ref_id(1).await, new.repeat(40));
            assert_eq!(
                f.publications
                    .head(&f.instance)
                    .await
                    .unwrap()
                    .unwrap()
                    .view_id,
                f.view(new).view_id
            );
        }
    }
    assert_eq!(f.counts().await, (2, 2, 2));
}

#[tokio::test]
async fn snapshot_publication_sqlite_expected_old_writers_do_not_overwrite() {
    let (_dir, f) = sqlite().await;
    competing_writers(&f).await;
}

async fn postgres() -> (Fixture, ConnectOptions) {
    let url = std::env::var("MEGA_SNAPSHOT_TEST_DATABASE_URL")
        .expect("explicit disposable PostgreSQL URL required");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(matches!(parsed.scheme(), "postgres" | "postgresql"));
    assert!(matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ));
    assert_eq!(parsed.path(), "/snapshot_test");
    let control = Database::connect(
        ConnectOptions::new(url.clone())
            .max_connections(1)
            .sqlx_logging(false)
            .to_owned(),
    )
    .await
    .unwrap();
    let schema = format!("snapshot_publication_{}", uuid::Uuid::new_v4().simple());
    control
        .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    let options = ConnectOptions::new(url)
        .max_connections(12)
        .sqlx_logging(false)
        .set_schema_search_path(schema.clone())
        .to_owned();
    let db = Database::connect(options.clone()).await.unwrap();
    jupiter_migrate::apply_migrations(&db, false).await.unwrap();
    println!("publication PostgreSQL schema retained: {schema}");
    (Fixture::new(BaseStorage::new(Arc::new(db))).await, options)
}

#[tokio::test]
#[ignore = "requires explicit disposable loopback MEGA_SNAPSHOT_TEST_DATABASE_URL"]
async fn snapshot_publication_postgres_lifecycle_and_reopen() {
    let (f, options) = postgres().await;
    lifecycle(&f).await;
    let reopened = PublicationStorage {
        base: BaseStorage::new(Arc::new(Database::connect(options).await.unwrap())),
    };
    assert_eq!(
        reopened.head(&f.instance).await.unwrap(),
        f.publications.head(&f.instance).await.unwrap()
    );
    let view = f.view("B");
    assert_eq!(
        reopened.view(&f.instance, &view.view_id).await.unwrap(),
        Some(view.canonical_bytes)
    );
    // Resolve every committed result on an independent connection after the
    // caller has discarded its finish response (no repeated ref mutation).
    for row in snapshot_operation::Entity::find()
        .all(f.refs.base.get_connection())
        .await
        .unwrap()
    {
        let request = PublicationRequest {
            instance_id: row.instance_id,
            actor_domain: row.actor_domain,
            operation_id: row.operation_id,
            request_digest: row.request_digest,
        };
        assert_eq!(
            reopened.receipt(&request).await.unwrap(),
            f.publications.receipt(&request).await.unwrap()
        );
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable loopback MEGA_SNAPSHOT_TEST_DATABASE_URL"]
async fn snapshot_publication_postgres_concurrent_duplicate_and_expected_old() {
    let (f, _) = postgres().await;
    duplicate_race(&f).await;
    let (f, _) = postgres().await;
    competing_writers(&f).await;
}

#[tokio::test]
#[ignore = "requires explicit disposable loopback MEGA_SNAPSHOT_TEST_DATABASE_URL"]
async fn snapshot_publication_postgres_external_epoch_change_fences_noop() {
    let (f, _) = postgres().await;
    let view = f.view("A");
    f.ready(f.request("bootstrap"), None)
        .await
        .finish(&view, "initial")
        .await
        .unwrap();
    let expected = f.publications.head(&f.instance).await.unwrap();
    let request = f.request("stale writer noop");
    let txn = f.ready(request.clone(), expected).await;
    // The pool supplies an independent connection; this fence commits outside
    // the stale writer's transaction while its operation reservation is held.
    namespace_head::Entity::update_many()
        .col_expr(namespace_head::Column::WriterEpoch, Expr::value(2i64))
        .filter(namespace_head::Column::InstanceId.eq(&f.instance))
        .exec(f.refs.base.get_connection())
        .await
        .unwrap();
    assert!(
        f.refs
            .update_ref_if_unchanged(
                42,
                "refs/heads/feature",
                &"a".repeat(40),
                &"f".repeat(40),
                txn.transaction()
            )
            .await
            .unwrap()
    );
    assert!(matches!(
        txn.finish(&view, "stale writer noop").await,
        Err(MegaError::Conflict(_))
    ));
    assert_eq!(
        f.publications
            .head(&f.instance)
            .await
            .unwrap()
            .unwrap()
            .writer_epoch,
        2
    );
    assert_eq!(f.ref_id(2).await, "a".repeat(40));
    assert!(f.publications.receipt(&request).await.unwrap().is_none());
    assert_eq!(f.counts().await, (1, 1, 1));
}
