mod config;
mod handlers;
mod keep_alive;
mod orion_deployer;
mod state;
mod vm_cleanup;
mod vm_manager;

use std::sync::Arc;

use axum::Router;
use state::AppState;
use tokio::signal::{ctrl_c, unix::SignalKind};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Gracefully shutdown VM and clear state on service termination signals.
///
/// Acquires the update lock with a short timeout so a slow in-flight
/// `/webhook` can't keep us racing forever (systemd would eventually
/// SIGKILL us and leave an orphan qemu). When the lock can't be taken
/// in time, we still proceed and rely on the pkill safety net below
/// to reap any qemu processes the racing create may have spawned.
async fn shutdown_vm(state: &AppState) {
    tracing::info!("[shutdown] Initiating VM shutdown");

    let _guard = state
        .try_lock_update(std::time::Duration::from_secs(10))
        .await;
    if _guard.is_none() {
        tracing::warn!(
            "[shutdown] timed out waiting for update lock; \
             proceeding and relying on pkill safety net"
        );
    }

    if let Some(machine) = state.get_machine().await {
        tracing::info!("[shutdown] VM found, calling shutdown...");
        match machine.shutdown().await {
            Ok(_) => tracing::info!("[shutdown] VM shutdown completed successfully"),
            Err(e) => tracing::error!("[shutdown] VM shutdown failed: {}", e),
        }
    } else {
        tracing::info!("[shutdown] No VM running");
    }
    state.clear_vm().await;

    // Reap any qemu process that escaped tracking — racing creates whose
    // KeepAliveMachine never made it into `state`, or processes left over
    // from a previous crashed run. Matches the same pattern we run at
    // startup so the next run begins clean even if SIGKILL hits us next.
    let pkill = tokio::process::Command::new("pkill")
        .args(["-9", "-f", "qemu-system-x86"])
        .output()
        .await;
    match pkill {
        Ok(out) if out.status.success() => {
            tracing::warn!("[shutdown] pkill reaped leftover qemu process(es)");
        }
        Ok(_) => tracing::info!("[shutdown] pkill found no leftover qemu"),
        Err(e) => tracing::error!("[shutdown] pkill failed: {e}"),
    }

    // Disk-side cleanup: even if Machine::drop ran, racing/aborted creates
    // can have left ~0.5–3 GB of overlay/seed on disk. Sweep here so we
    // don't accumulate gigabytes across signal-driven restarts.
    vm_cleanup::sweep_stale_runs().await;

    tracing::info!("[shutdown] State cleared");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting orion-scheduler service");

    // Cleanup any residual processes from previous runs, then sweep their
    // on-disk run directories. The pkill alone can leave 0.5–3 GB per VM
    // behind under ~/.local/share/qlean/runs/ because qlean only deletes
    // those dirs from `Machine::drop`, which never runs on SIGKILL/abort.
    tracing::info!("[startup] Checking for residual QEMU processes");
    tokio::process::Command::new("pkill")
        .args(["-9", "-f", "qemu-system-x86"])
        .output()
        .await
        .ok();
    vm_cleanup::sweep_stale_runs().await;

    // Load target configuration.
    //
    // If `CONFIG_PATH` is set we respect it verbatim — the operator has been
    // explicit and we should not silently look elsewhere. Otherwise we walk
    // a short candidate list (cwd → exe dir → crate root) so common dev
    // invocations like `cargo run --bin orion-scheduler` from the workspace
    // root find the crate-local `target_config.json` instead of dying with
    // a bare `No such file or directory` and no path context.
    let config_path: std::path::PathBuf = match std::env::var_os("CONFIG_PATH") {
        Some(explicit) => std::path::PathBuf::from(explicit),
        None => config::default_config_path().ok_or_else(|| {
            let candidates = config::default_config_candidates()
                .into_iter()
                .map(|p| format!("  - {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::anyhow!(
                "could not locate target_config.json; set CONFIG_PATH or place the file at one of:\n{candidates}"
            )
        })?,
    };
    tracing::info!("[startup] Loading config from: {}", config_path.display());
    let config = config::Config::load(&config_path).await?;
    let config = Arc::new(tokio::sync::RwLock::new(config));
    tracing::info!(
        "[startup] Config loaded, default_image path: {}",
        config.read().await.default_image().image_path
    );

    // Create shared state
    let state = Arc::new(AppState::new(config));

    // Build router - use separate routes for GET and POST
    let app = Router::new()
        .route(
            "/webhook",
            axum::routing::get(handlers::webhook_get_handler),
        )
        .route(
            "/webhook",
            axum::routing::post(handlers::webhook_post_handler),
        )
        .route("/health", axum::routing::get(handlers::health_handler))
        .route("/status", axum::routing::get(handlers::status_handler))
        .route(
            "/logs/orion/stream",
            axum::routing::get(handlers::logs_stream_handler),
        )
        .route(
            "/scorpio/status",
            axum::routing::get(handlers::scorpio_status_handler),
        )
        .route(
            "/scorpio/config",
            axum::routing::get(handlers::scorpio_config_handler),
        )
        .route("/shutdown", axum::routing::post(handlers::shutdown_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state.clone());

    // Start server
    let addr = "0.0.0.0:8080";
    tracing::info!("[startup] Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Handle termination signals: stop VM and server
    let term_shutdown_state = state.clone();
    let term_shutdown_signal = async move {
        if let Some(()) = tokio::signal::unix::signal(SignalKind::terminate())
            .unwrap()
            .recv()
            .await
        {
            tracing::info!("[shutdown] Received SIGTERM");
            shutdown_vm(&term_shutdown_state).await;
        }
    };

    let quit_shutdown_state = state.clone();
    let quit_shutdown_signal = async move {
        if let Some(()) = tokio::signal::unix::signal(SignalKind::quit())
            .unwrap()
            .recv()
            .await
        {
            tracing::info!("[shutdown] Received SIGQUIT");
            shutdown_vm(&quit_shutdown_state).await;
        }
    };

    // Handle Ctrl+C: stop VM and server
    let ctrl_c_shutdown_state = state.clone();
    let ctrl_c_signal = async move {
        match ctrl_c().await {
            Ok(()) => {
                tracing::info!("[shutdown] Received SIGINT (Ctrl+C)");
                shutdown_vm(&ctrl_c_shutdown_state).await;
            }
            Err(e) => tracing::error!("[shutdown] Ctrl+C handler error: {}", e),
        }
    };

    tracing::info!("[startup] Server running. Use /shutdown to stop VM only");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::select! {
                _ = ctrl_c_signal => {}
                _ = term_shutdown_signal => {}
                _ = quit_shutdown_signal => {}
            }
        })
        .await?;

    tracing::info!("[shutdown] Server exiting");
    Ok(())
}
