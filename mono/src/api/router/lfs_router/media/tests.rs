use std::sync::Arc;

use axum::{
    Router,
    body::to_bytes,
    http::{HeaderMap, Request},
    routing::{get, post},
};
use ceres::lfs::{
    handler,
    lfs_structs::{BatchRequest, BatchResponse, RequestObject},
};
use common::config::{LocalConfig, ObjectStorageBackend, ObjectStorageConfig};
use io_orbit::factory::ObjectStorageFactory;
use jupiter::storage::{
    base_storage::{BaseStorage, StorageConnector},
    lfs_db_storage::LfsDbStorage,
};
use sea_orm::{ConnectionTrait, Database};
use tower::{Layer, ServiceExt};

use super::*;

async fn fixture() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE lfs_objects (oid TEXT PRIMARY KEY, size BIGINT NOT NULL, exist BOOLEAN NOT NULL)").await.unwrap();
    db.execute_unprepared("CREATE TABLE access_token (id BIGINT PRIMARY KEY, campsite_user_id TEXT NOT NULL, token TEXT NOT NULL, created_at TEXT NOT NULL, github_login TEXT)").await.unwrap();
    db.execute_unprepared("INSERT INTO access_token VALUES (1, 'alice', 'test-alice', '2026-01-01 00:00:00', NULL), (2, 'bob', 'test-bob', '2026-01-01 00:00:00', NULL)").await.unwrap();
    let base = BaseStorage::new(Arc::new(db));
    let config = ObjectStorageConfig {
        storage_type: ObjectStorageBackend::Local,
        local: LocalConfig {
            root_dir: dir.path().to_string_lossy().into_owned(),
        },
        ..Default::default()
    };
    let state = MediaState {
        lfs: LfsService {
            lfs_storage: LfsDbStorage { base: base.clone() },
            obj_storage: ObjectStorageFactory::build(&config).await.unwrap(),
        },
        users: UserStorage { base },
    };
    let media: Router = router::<MediaState>().with_state(state.clone()).into();
    // The fixture uses the real basic handlers too, so Libra can exercise its
    // ordinary Batch -> upload/download entry points rather than bypass them.
    let basic = Router::new()
        .route("/info/lfs/objects/batch", post(basic_batch))
        .route(
            "/info/lfs/objects/{oid}",
            get(basic_download).put(basic_upload),
        )
        .with_state(state);
    let app = Router::new()
        .nest("/info/lfs/libra/media/v1", media)
        .merge(basic);
    let app = tower::util::MapRequestLayer::new(
        crate::server::http_server::rewrite_lfs_request_uri::<Body>,
    )
    .layer(app);
    (dir, Router::new().fallback_service(app))
}

async fn basic_batch(
    State(state): State<MediaState>,
    headers: HeaderMap,
    Json(request): Json<BatchRequest>,
) -> Json<BatchResponse> {
    let host = headers.get("host").unwrap().to_str().unwrap();
    Json(
        handler::lfs_process_batch(&state.lfs, request, &format!("http://{host}"))
            .await
            .unwrap(),
    )
}

async fn basic_download(State(state): State<MediaState>, Path(oid): Path<String>) -> Body {
    Body::from_stream(handler::lfs_download_object(state.lfs, oid).await.unwrap())
}

async fn basic_upload(
    State(state): State<MediaState>,
    Path(oid): Path<String>,
    request: Request<Body>,
) -> StatusCode {
    let bytes = to_bytes(request.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    handler::lfs_upload_object(
        &state.lfs,
        &RequestObject {
            oid,
            size: bytes.len() as i64,
            ..Default::default()
        },
        bytes.to_vec(),
    )
    .await
    .unwrap();
    StatusCode::OK
}

#[tokio::test]
async fn media_router_requires_token_and_preserves_repository_scope() {
    let (_dir, app) = fixture().await;
    let path = "/project/demo.git/info/lfs/libra/media/v1/capabilities";
    for token in [None, Some("unknown-token")] {
        let mut request = Request::builder().uri(path);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("Authorization", "Bearer test-alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(value["chunk_algorithms"][0], "fastcdc-v1");
    let path = format!(
        "/project/demo.git/info/lfs/libra/media/v1/manifests/by-media/{}",
        "a".repeat(64)
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .header("Authorization", "Bearer test-bob")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn media_http_errors_preserve_response_contract() {
    let too_large = body(Request::new(Body::from("ab")), 1).await.unwrap_err();
    for (error, status, content_type, message) in [
        (
            MediaHttpError::from(MediaError::Invalid("bad manifest".into())),
            StatusCode::BAD_REQUEST,
            LFS_CONTENT_TYPE,
            "Invalid media request: bad manifest",
        ),
        (
            MediaHttpError::from(MediaError::NotFound),
            StatusCode::NOT_FOUND,
            LFS_CONTENT_TYPE,
            "Media object not found",
        ),
        (
            MediaHttpError::from(MediaError::Conflict),
            StatusCode::CONFLICT,
            LFS_CONTENT_TYPE,
            "Media manifest conflicts with the finalized object",
        ),
        (
            MediaHttpError::from(MediaError::Storage("private backend diagnostics".into())),
            StatusCode::INTERNAL_SERVER_ERROR,
            LFS_CONTENT_TYPE,
            "media storage operation failed",
        ),
        (
            too_large,
            StatusCode::PAYLOAD_TOO_LARGE,
            "application/json",
            "media body exceeds limit or cannot be read",
        ),
    ] {
        let response = error.into_response();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["Content-Type"], content_type);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"message": message}));
    }
}

#[tokio::test]
async fn media_router_rejects_invalid_manifest_bodies() {
    let (_dir, app) = fixture().await;
    let unreadable = Body::from_stream(futures::stream::once(async {
        Err::<Vec<u8>, _>(std::io::Error::other("test body failure"))
    }));
    for (body, status, content_type, message) in [
        (
            Body::from("{"),
            StatusCode::BAD_REQUEST,
            LFS_CONTENT_TYPE,
            "Invalid media request: malformed manifest JSON",
        ),
        (
            unreadable,
            StatusCode::PAYLOAD_TOO_LARGE,
            "application/json",
            "media body exceeds limit or cannot be read",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/project/demo.git/info/lfs/libra/media/v1/manifests")
                    .header("Authorization", "Bearer test-alice")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["Content-Type"], content_type);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"message": message}));
    }
}

/// Serves the actual production media router plus token validation against an
/// isolated SQLite database for Libra's ignored cross-repository HTTP test.
#[tokio::test]
#[ignore = "run with MEGA_FASTCDC_READY_FILE for Libra/Mega interop"]
async fn serve_libra_interop() {
    let ready =
        std::env::var("MEGA_FASTCDC_READY_FILE").expect("MEGA_FASTCDC_READY_FILE is required");
    let (_dir, app) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/project/demo.git/info/lfs/",
        listener.local_addr().unwrap()
    );
    std::fs::write(
        ready,
        serde_json::to_vec(&serde_json::json!({"lfs_url":url,"token":"test-alice"})).unwrap(),
    )
    .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let stop = Arc::new(tokio::sync::Mutex::new(Some(stop)));
    let app = app.route(
        "/__test/stop",
        axum::routing::post(move || {
            let stop = stop.clone();
            async move {
                if let Some(stop) = stop.lock().await.take() {
                    let _ = stop.send(());
                }
                StatusCode::NO_CONTENT
            }
        }),
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(600), stopped).await;
        })
        .await
        .unwrap();
}
