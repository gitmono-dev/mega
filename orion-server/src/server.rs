use std::{env, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use common::{
    config::{
        Config,
        loader::{ConfigInput, ConfigLoader},
    },
    errors::MegaError,
};
use http::{HeaderValue, Method};
use io_orbit::factory::ObjectStorageFactory;
use sea_orm::Database;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    api,
    api_doc::ApiDoc,
    app_state::AppState,
    buck2::set_mono_base_url,
    log::{
        log_service::LogService,
        store::{LogStore, io_orbit_store::IoOrbitLogStore, local_log_store, noop_log_store},
    },
};

pub async fn init_log_service(config: Config) -> Result<LogService, MegaError> {
    // Read buffer size from environment, defaulting to 4096 if unset or invalid

    let orion_server = config
        .orion_server
        .as_ref()
        .ok_or_else(|| MegaError::Other("orion_server config section missing".to_string()))?;

    let buffer = orion_server.log_stream_buffer;
    let log_store_mode = orion_server.logger_storage_mode.clone();
    let build_log_dir = orion_server.build_log_dir.clone();

    let noop_log_store: Arc<dyn LogStore> = Arc::new(noop_log_store::NoopLogStore::new());

    let (local_log_store, cloud_log_store, cloud_upload_enabled): (
        Arc<dyn LogStore>,
        Arc<dyn LogStore>,
        bool,
    ) = match log_store_mode.as_str() {
        "none" => (noop_log_store.clone(), noop_log_store.clone(), false),
        "local" => (
            Arc::new(local_log_store::LocalLogStore::new(&build_log_dir)),
            noop_log_store.clone(),
            false,
        ),
        "mix" => {
            let object_store_wrapper = ObjectStorageFactory::build(&config.object_storage).await?;
            (
                Arc::new(local_log_store::LocalLogStore::new(&build_log_dir)),
                Arc::new(IoOrbitLogStore::new(object_store_wrapper)),
                true,
            )
        }
        other => panic!(
            "Unsupported LOGGER_STORAGE_TYPE: {}. Supported values: [local, none, s3]",
            other
        ),
    };

    tracing::info!(
        storage_type = %log_store_mode,
        cloud_upload_enabled,
        build_log_dir = %build_log_dir,
        "Initialized log service storage configuration"
    );

    // Create the LogService
    Ok(LogService::new(
        local_log_store,
        cloud_log_store,
        buffer,
        cloud_upload_enabled,
    ))
}

async fn load_orion_config() -> Result<Config, MegaError> {
    let input = ConfigInput {
        cli_path: None,
        env_path: env::var_os("MEGA_CONFIG").map(PathBuf::from),
    };

    let loaded = ConfigLoader::new(input).load()?;
    tracing::info!(
        source = ?loaded.source,
        path = %loaded.path.display(),
        "config loaded"
    );

    Config::new(loaded.path.to_str().ok_or_else(|| {
        MegaError::Other(format!(
            "Config path contains invalid UTF-8: {:?}",
            loaded.path
        ))
    })?)
}

/// Starts the Orion server with the specified port
/// Initializes database connection, sets up routes, and starts health check tasks
pub async fn start_server() {
    let config = load_orion_config().await.unwrap_or_else(|e| {
        eprintln!("Failed to load config: {}", e);
        std::process::exit(1);
    });

    let Some(orion_server_config) = config.orion_server.clone() else {
        eprintln!("Missing `orion_server` section in config");
        std::process::exit(1);
    };
    let conn = Database::connect(orion_server_config.db_url)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Database connection failed: {e}");
            std::process::exit(1);
        });

    let port = orion_server_config.port;

    // Set mono base URL for buck2 file/blob API (from config).
    set_mono_base_url(orion_server_config.monobase_url.clone());

    // Derive allowed CORS origins from oauth config (or its default when missing).
    // Do this before `init_log_service(config)` consumes `config`.
    let oauth_cfg = config.oauth.clone();

    // Initialize the LogService, then create the application state with the same
    // LogService instance. Log persistence now happens inline on the build-output
    // path (local) plus a background retryable cloud upload on completion, so no
    // broadcast-watcher task is needed.
    let log_service = init_log_service(config).await.unwrap_or_else(|e| {
        eprintln!("Failed to initialize LogService: {}", e);
        std::process::exit(1);
    });

    let state = AppState::new(conn, None, log_service);

    // Reconcile builds orphaned by a previous instance (e.g. left stuck in
    // `Building` after a crash/restart) before we accept new work or workers.
    state.scheduler.reconcile_orphaned_builds_on_startup().await;

    // Start background health check task
    tokio::spawn(start_health_check_task(state.clone()));

    // Start queue manager
    tokio::spawn(state.clone().start_queue_manager());

    // Start background dp operation
    state.start_background_tasks();

    let origins: Vec<HeaderValue> = oauth_cfg
        .allowed_cors_origins
        .into_iter()
        .filter_map(|x| {
            let v = x.trim();
            if v.is_empty() {
                return None;
            }
            match v.parse::<HeaderValue>() {
                Ok(h) => Some(h),
                Err(e) => {
                    tracing::warn!("Invalid CORS origin header value '{v}': {e}");
                    None
                }
            }
        })
        .collect();

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .merge(api::routers())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(
            ServiceBuilder::new().layer(
                CorsLayer::new()
                    .allow_origin(origins)
                    .allow_headers(vec![
                        http::header::AUTHORIZATION,
                        http::header::CONTENT_TYPE,
                    ])
                    .allow_methods([
                        Method::GET,
                        Method::POST,
                        Method::OPTIONS,
                        Method::DELETE,
                        Method::PUT,
                    ])
                    .allow_credentials(true),
            ),
        );

    tracing::info!("Listening on port {}", port);
    let addr = tokio::net::TcpListener::bind(&format!("0.0.0.0:{port}"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind 0.0.0.0:{port}: {e}");
            std::process::exit(1);
        });
    axum::serve(
        addr,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap_or_else(|e| {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    });
}

/// Background task that monitors worker health and handles timeouts
/// Removes dead workers and marks their in-flight builds as interrupted
async fn start_health_check_task(state: AppState) {
    let health_check_interval = Duration::from_secs(30);
    let worker_timeout = Duration::from_secs(90);

    tracing::info!(
        "Health check task started. Interval: {:?}, Worker timeout: {:?}",
        health_check_interval,
        worker_timeout
    );

    loop {
        tokio::time::sleep(health_check_interval).await;
        tracing::debug!("Running health check...");

        let mut dead_workers = Vec::new();
        let now = chrono::Utc::now();

        // Find workers that haven't sent heartbeat within timeout period
        for entry in state.scheduler.workers.iter() {
            if now.signed_duration_since(entry.value().last_heartbeat)
                > chrono::Duration::seconds(worker_timeout.as_secs() as i64)
            {
                dead_workers.push(entry.key().clone());
            }
        }

        if dead_workers.is_empty() {
            continue;
        }

        tracing::warn!("Found dead workers: {:?}", dead_workers);

        for worker_id in dead_workers {
            if let Some((_, worker_info)) = state.scheduler.workers.remove(&worker_id) {
                tracing::info!("Removed dead worker: {}", worker_id);
                state
                    .scheduler
                    .handle_worker_gone(worker_info, "heartbeat timed out")
                    .await;
            }
        }
    }
}
