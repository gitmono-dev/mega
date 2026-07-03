use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Result;
use axum::{
    Router, ServiceExt,
    body::Body,
    extract::FromRef,
    http::{self, Request, Uri},
    middleware,
    response::Response,
    routing::any,
};
use ceres::{
    TransportRuntime,
    application::{api_service::cache::GitObjectCache, artifact::ArtifactApplicationService},
};
use common::errors::ProtocolError;
use http::{HeaderName, HeaderValue, Method};
use orion_client::OrionBuildClient;
use orion_scheduler_client::OrionSchedulerClient;
use saturn::entitystore::EntityStore;
use time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower::{Layer, ServiceBuilder};
use tower_http::{cors::CorsLayer, decompression::RequestDecompressionLayer, trace::TraceLayer};
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use super::super::notification::{EmailDispatcher, EmailMailer};
use crate::{
    api::{
        MonoApiServiceState,
        api_doc::ApiDoc,
        api_router::{self},
        guard::cedar_guard::cedar_guard,
        oauth::{
            api_store::OAuthApiStore, campsite_store::CampsiteApiStore,
            tinyship_store::TinyshipApiStore,
        },
        router::lfs_router,
    },
    bootstrap::AppContext,
    email::{NoopMailer, SmtpMailer},
    git_protocol::InfoRefsParams,
    orion_build_dispatch::OrionBuildDispatch,
    server::{CommonHttpOptions, trace_context},
};

pub fn remove_git_suffix(full_path: &str, git_suffix: &str) -> PathBuf {
    PathBuf::from(full_path.replace(".git", "").replace(git_suffix, ""))
}

fn is_disallowed_root_repo_path(full_path: &str) -> bool {
    matches!(
        full_path.trim_start_matches('/').split('/').next(),
        Some("third-party.git")
    )
}

/// Spawns a background task to clean up expired Buck upload sessions.
///
/// Returns `None` if cleanup is disabled in configuration.
fn spawn_cleanup_task(ctx: AppContext, token: CancellationToken) -> Option<JoinHandle<()>> {
    let config = ctx.storage.config();
    let buck_config = config.buck.clone().unwrap_or_default();

    if !buck_config.enable_session_cleanup {
        return None;
    }

    let cleanup_storage = ctx.storage.clone();
    let cleanup_interval = buck_config.cleanup_interval;
    let retention_days = buck_config.completed_retention_days;

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(cleanup_interval));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tracing::info!(
            "Buck upload session cleanup task started (interval: {}s, retention: {}d)",
            cleanup_interval,
            retention_days
        );

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match cleanup_storage
                        .buck_storage()
                        .delete_expired_sessions(retention_days)
                        .await
                    {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(
                                    "Buck upload cleanup: deleted {} expired sessions",
                                    count
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Buck upload cleanup failed: {}. Will retry in next interval.",
                                e
                            );
                        }
                    }
                }
                _ = token.cancelled() => {
                    tracing::info!("Buck upload cleanup task received shutdown signal");
                    break;
                }
            }
        }

        tracing::info!("Buck upload cleanup task stopped gracefully");
    }))
}

/// Spawns a background task to deliver pending email jobs
fn spawn_email_dispatcher_task(ctx: AppContext, token: CancellationToken) -> JoinHandle<()> {
    // Build a mailer (Default to NoopMailer if config missing or invalid)
    let cfg = ctx.storage.config();
    let mailer: Arc<dyn EmailMailer> = if let Some(mail_cfg) = &cfg.mail {
        match SmtpMailer::new(mail_cfg) {
            Ok(m) => Arc::new(m),
            Err(e) => {
                tracing::warn!("Failed to initialize SMTP mailer, falling back to noop: {e:?}");
                Arc::new(NoopMailer)
            }
        }
    } else {
        Arc::new(NoopMailer)
    };

    let stg = ctx.storage.notification_storage();
    let dispatcher = EmailDispatcher::new(stg, mailer);

    tokio::spawn(async move {
        dispatcher.run(token).await;
    })
}

/// Background GC for `artifact_objects` with no manifest references (`docs/artifacts-protocol.md` §10.6).
fn spawn_artifact_gc_task(ctx: AppContext, token: CancellationToken) -> Option<JoinHandle<()>> {
    let cfg = ctx.storage.config().artifacts_gc.clone();
    if !cfg.enable {
        return None;
    }

    let interval_secs = cfg.interval_secs.max(1);
    let grace_secs = cfg.grace_secs;
    let batch_limit = cfg.batch_limit.max(1);
    let service = ArtifactApplicationService::from_storage(&ctx.storage);

    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tracing::info!(
            "artifact_objects GC task started (interval={interval_secs}s, grace={grace_secs}s, batch_limit={batch_limit})"
        );

        let grace = std::time::Duration::from_secs(grace_secs);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match service
                        .gc_unreferenced_once(grace, batch_limit)
                        .await
                    {
                        Ok(s) if s.deleted > 0 || s.candidates > 0 => {
                            tracing::info!(
                                candidates = s.candidates,
                                deleted = s.deleted,
                                skipped_still_referenced = s.skipped_still_referenced,
                                storage_delete_errors = s.storage_delete_errors,
                                db_delete_errors = s.db_delete_errors,
                                "artifact_objects GC tick"
                            );
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!(error = %e, "artifact_objects GC tick failed"),
                    }
                }
                _ = token.cancelled() => {
                    tracing::info!("artifact_objects GC task received shutdown signal");
                    break;
                }
            }
        }

        tracing::info!("artifact_objects GC task stopped gracefully");
    }))
}

/// Returns a future that completes when the cancellation token is triggered.
async fn shutdown_signal(token: CancellationToken) {
    token.cancelled().await;
}

pub async fn start_http(ctx: AppContext, options: CommonHttpOptions) {
    let CommonHttpOptions { host, port } = options.clone();

    let middleware = tower::util::MapRequestLayer::new(rewrite_lfs_request_uri::<Body>);

    let shutdown_token = CancellationToken::new();
    let cleanup_handle = spawn_cleanup_task(ctx.clone(), shutdown_token.clone());
    let dispatcher_handle = spawn_email_dispatcher_task(ctx.clone(), shutdown_token.clone());
    let artifact_gc_handle = spawn_artifact_gc_task(ctx.clone(), shutdown_token.clone());
    let server_token = shutdown_token.clone();

    let app = app(ctx, host.clone(), port).await;
    let app_with_middleware = middleware.layer(app);

    let server_url = format!("{host}:{port}");
    let addr = SocketAddr::from_str(&server_url).unwrap();
    tracing::info!("HTTP server started up!");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    let server_future = axum::serve(listener, app_with_middleware.into_make_service())
        .with_graceful_shutdown(shutdown_signal(server_token));

    let server_handle = tokio::spawn(async move {
        if let Err(e) = server_future.await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    tokio::pin!(server_handle);

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received shutdown signal (Ctrl+C), starting graceful shutdown...");
        }
        result = server_handle.as_mut() => {
            if let Err(e) = result {
                tracing::error!("HTTP server unexpectedly stopped: {}", e);
            }
            tracing::info!("HTTP server stopped, initiating shutdown...");
        }
    }

    tracing::info!("Broadcasting shutdown signal to all tasks...");
    shutdown_token.cancel();

    let (cleanup_result, dispatcher_result, artifact_gc_result, server_result) = tokio::join!(
        async {
            if let Some(handle) = cleanup_handle {
                match tokio::time::timeout(std::time::Duration::from_secs(30), handle).await {
                    Ok(Ok(_)) => {
                        tracing::info!("Cleanup task stopped successfully");
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Cleanup task panicked: {}", e);
                        Err(())
                    }
                    Err(_) => {
                        // Timeout indicates potential deadlock or extremely slow I/O.
                        tracing::error!(
                            "Cleanup task did not stop within 30s timeout. \
                            This may indicate a deadlock or extremely slow I/O. \
                            The task will be detached and may continue running. \
                            Operators: check DB/Redis connectivity and long-running I/O; \
                            consider increasing cleanup_interval if workloads are heavy."
                        );
                        Err(())
                    }
                }
            } else {
                Ok(())
            }
        },
        async {
            match tokio::time::timeout(std::time::Duration::from_secs(30), dispatcher_handle).await
            {
                Ok(Ok(_)) => {
                    tracing::info!("Email dispatcher task stopped successfully");
                    Ok(())
                }
                Ok(Err(e)) => {
                    tracing::error!("Email dispatcher task panicked: {}", e);
                    Err(())
                }
                Err(_) => {
                    tracing::error!(
                        "Email dispatcher did not stop within 30s timeout. The task will be detached."
                    );
                    Err(())
                }
            }
        },
        async {
            if let Some(handle) = artifact_gc_handle {
                match tokio::time::timeout(std::time::Duration::from_secs(30), handle).await {
                    Ok(Ok(_)) => {
                        tracing::info!("artifact_objects GC task stopped successfully");
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        tracing::error!("artifact_objects GC task panicked: {}", e);
                        Err(())
                    }
                    Err(_) => {
                        tracing::error!(
                            "artifact_objects GC task did not stop within 30s timeout. The task will be detached."
                        );
                        Err(())
                    }
                }
            } else {
                Ok(())
            }
        },
        async {
            match server_handle.as_mut().await {
                Ok(_) => {
                    tracing::info!("HTTP server stopped gracefully");
                    Ok(())
                }
                Err(e) => {
                    tracing::error!("HTTP server join error: {}", e);
                    Err(())
                }
            }
        }
    );

    match (
        cleanup_result,
        dispatcher_result,
        artifact_gc_result,
        server_result,
    ) {
        (Ok(_), Ok(_), Ok(_), Ok(_)) => {
            tracing::info!("Graceful shutdown completed successfully");
        }
        _ => {
            tracing::warn!("Graceful shutdown completed with some errors");
        }
    }
}

/// This is the main entry for the mono server.
/// It is responsible for creating the main router and setting up the necessary middleware.
///
/// The main router is composed of three nested routers:
/// 1. The LFS router nested in the `/`:
///   - GET or PUT `/objects/:object_id`
///   - GET or PUT `/locks`
///   - POST       `/locks/verify`
///   - POST       `/locks/:id/unlock`
///   - GET        `/objects/:object_id/chunks/:chunk_id`
///   - POST       `/objects/batch`
/// 2. The API router nested in the `/api/v1`:
///   - GET        `/api/v1/status`
///   - POST       `/api/v1/create-file`
///   - GET        `/api/v1/latest-commit`
///   - GET        `/api/v1/tree/commit-info`
///   - GET        `/api/v1/tree`
///   - GET        `/api/v1/blob`
///   - GET        `/api/v1/file/blob/:object_id`
///   - GET        `/api/v1/file/tree`
///   - GET        `/api/v1/path-can-clone`
/// 3. The OAuth router nested in the `/auth`:
///   - GET        `/auth/github`
///   - GET        `/auth/authorized`
///   - GET        `/auth/logout`
/// 4. The other routers for the git protocol:
///   - GET        end of `Regex::new(r"/info/refs$")`
///   - POST       end of `Regex::new(r"/git-upload-pack$")`
///   - POST       end of `Regex::new(r"/git-receive-pack$")`
pub async fn app(ctx: AppContext, host: String, port: u16) -> Router {
    let storage = ctx.storage;
    let config = storage.config();

    let oauth_config = config.oauth.clone();
    let git_object_cache = Arc::new(GitObjectCache {
        connection: ctx.connection.clone(),
        prefix: "git-object-rkyv:v1".to_string(),
    });

    let orion_client = Arc::new(OrionBuildClient::new(storage.config().build.clone()));
    let build_dispatch = OrionBuildDispatch::new(orion_client.clone()).into_arc();
    let orion_scheduler_client =
        Arc::new(OrionSchedulerClient::new(storage.config().build.clone()));

    let api_state = MonoApiServiceState::new(
        storage.clone(),
        git_object_cache,
        build_dispatch,
        Some(match oauth_config.api_store_backend {
            common::config::OauthApiStoreBackend::Campsite => {
                OAuthApiStore::Campsite(CampsiteApiStore::new(oauth_config.campsite_api_domain))
            }
            common::config::OauthApiStoreBackend::Tinyship => {
                OAuthApiStore::Tinyship(TinyshipApiStore::new(oauth_config.tinyship_api_domain))
            }
        }),
        format!("http://{host}:{port}"),
        EntityStore::new(),
        Some(orion_scheduler_client),
    );

    let origins: Vec<HeaderValue> = oauth_config
        .allowed_cors_origins
        .into_iter()
        .map(|x| x.trim().parse::<HeaderValue>().unwrap())
        .collect();

    // add RequestDecompressionLayer for handle gzip encode
    // add TraceLayer for log record
    // add CorsLayer to add cors header
    // add SessionManagerLayer for session management
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false) // Set to true in production with HTTPS
        .with_expiry(Expiry::OnInactivity(Duration::seconds(3600))); // 1 hour of inactivity

    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(lfs_router::routers().with_state(api_state.clone()))
        .nest(
            "/api/v1",
            api_router::routers()
                .with_state(api_state.clone())
                .route_layer(middleware::from_fn_with_state(
                    api_state.clone(),
                    cedar_guard,
                )),
        )
        // .nest("/auth", oauth::routers().with_state(api_state.clone()))
        // Using Regular Expressions for Path Matching in Protocol
        .route(
            "/{*path}",
            any({
                let api_state = api_state.clone();
                move |req: Request<Body>| async move {
                    match handle_smart_protocol(
                        req,
                        Arc::new(TransportRuntime::from_ref(&api_state)),
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(err) => crate::git_protocol::protocol_error::into_response(err),
                    }
                }
            }),
        )
        .layer(
            ServiceBuilder::new().layer(session_layer).layer(
                CorsLayer::new()
                    .allow_origin(origins)
                    .allow_headers(vec![
                        http::header::AUTHORIZATION,
                        http::header::CONTENT_TYPE,
                        HeaderName::from_static("x-request-id"),
                        HeaderName::from_static("x-trace-id"),
                    ])
                    .expose_headers(vec![HeaderName::from_static("x-request-id")])
                    .allow_methods([
                        Method::GET,
                        Method::POST,
                        Method::OPTIONS,
                        Method::DELETE,
                        Method::PUT,
                    ])
                    .allow_credentials(true),
            ),
        )
        .layer(TraceLayer::new_for_http().make_span_with(trace_context::http_request_span))
        .layer(RequestDecompressionLayer::new())
        .layer(middleware::from_fn(trace_context::inject_trace_context))
        .with_state(api_state.clone())
        .split_for_parts();

    // Register /info/lfs paths for runtime compatibility (not in OpenAPI)
    // Convert OpenApiRouter to Router to avoid including /info/lfs in OpenAPI docs
    let info_lfs_router: Router = lfs_router::lfs_routes()
        .with_state(api_state.clone())
        .into();

    router
        .nest("/info/lfs", info_lfs_router)
        .merge(SwaggerUi::new("/swagger-ui").url("/api/openapi.json", api))
}

fn rewrite_lfs_request_uri<B>(mut req: Request<B>) -> Request<B> {
    let full_path = req.uri().path();

    if let Some(pos) = full_path.rfind("/info/lfs/") {
        let lfs_subpath = &full_path[pos..];

        let new_path_and_query = if let Some(query) = req.uri().query() {
            format!("{}?{}", lfs_subpath, query)
        } else {
            lfs_subpath.to_owned()
        };

        let new_uri = match Uri::builder().path_and_query(&new_path_and_query).build() {
            Ok(uri) => uri,
            Err(e) => {
                tracing::warn!(
                    "Failed to rewrite LFS URI: {}, error: {}",
                    new_path_and_query,
                    e
                );
                // Return the request unchanged, let downstream handlers deal with it
                return req;
            }
        };

        tracing::debug!("rewrite: old uri {:?}", req.uri());
        *req.uri_mut() = new_uri;
        tracing::debug!("rewrite: new uri {:?}", req.uri());
    }
    req
}

async fn handle_smart_protocol(
    req: Request<Body>,
    state: Arc<TransportRuntime>,
) -> Result<Response, ProtocolError> {
    let full_path = req.uri().path();
    if is_disallowed_root_repo_path(full_path) {
        return Err(ProtocolError::InvalidInput(
            "Repository third-party.git is not supported".to_string(),
        ));
    }
    if full_path.ends_with("/info/refs") && req.method().eq(&Method::GET) {
        let repo_path = remove_git_suffix(full_path, "/info/refs");
        let uri = req.uri();
        let query_str = uri.query().unwrap_or("");
        let params: InfoRefsParams = serde_urlencoded::from_str(query_str).unwrap();
        crate::git_protocol::http::git_info_refs(&state, params, repo_path).await
    } else if full_path.ends_with("/git-upload-pack") && req.method().eq(&Method::POST) {
        let repo_path = remove_git_suffix(full_path, "/git-upload-pack");
        crate::git_protocol::http::git_upload_pack(&state, req, repo_path).await
    } else if full_path.ends_with("/git-receive-pack") && req.method().eq(&Method::POST) {
        let repo_path = remove_git_suffix(full_path, "/git-receive-pack");
        crate::git_protocol::http::git_receive_pack(&state, req, repo_path).await
    } else {
        Ok(Response::builder()
            .status(404)
            .body(Body::from("Operation not supported"))
            .unwrap())
    }
}

#[cfg(test)]
mod tests {
    use http::Request;

    use super::*;

    #[test]
    fn test_disallow_third_party_git_root_repo() {
        assert!(is_disallowed_root_repo_path("/third-party.git/info/refs"));
        assert!(is_disallowed_root_repo_path(
            "/third-party.git/git-receive-pack"
        ));
        assert!(!is_disallowed_root_repo_path(
            "/third-party/test.git/info/refs"
        ));
        assert!(!is_disallowed_root_repo_path("/project.git/info/refs"));
    }

    #[test]
    fn test_rewrite_lfs_uri_basic() {
        let req = Request::builder()
            .uri("/repo/a/b/info/lfs/objects/123")
            .body(())
            .unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(new_req.uri().path(), "/info/lfs/objects/123");
    }

    #[test]
    fn test_rewrite_keeps_query_string() {
        let req = Request::builder()
            .uri("/repo/a/info/lfs/locks?token=abc123")
            .body(())
            .unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(
            new_req.uri().path_and_query().unwrap().to_string(),
            "/info/lfs/locks?token=abc123"
        );
    }

    #[test]
    fn test_no_rewrite_when_no_lfs_prefix() {
        let req = Request::builder().uri("/not-lfs-path").body(()).unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(new_req.uri().path(), "/not-lfs-path");
    }

    #[test]
    fn test_rewrite_with_trailing_slash() {
        let req = Request::builder()
            .uri("/repo/info/lfs/locks/")
            .body(())
            .unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(new_req.uri().path(), "/info/lfs/locks/");
    }

    #[test]
    fn test_rewrite_complex_path() {
        let req = Request::builder()
            .uri("/a/b/c/info/lfs/objects/abc/def/ghi")
            .body(())
            .unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(new_req.uri().path(), "/info/lfs/objects/abc/def/ghi");
    }

    #[test]
    fn test_rewrite_when_repo_path_contains_info_lfs() {
        let req = Request::builder()
            .uri("/repos/info/lfs/info/lfs/objects/123")
            .body(())
            .unwrap();

        let new_req = rewrite_lfs_request_uri(req);

        assert_eq!(new_req.uri().path(), "/info/lfs/objects/123");
    }
}
