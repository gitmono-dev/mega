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

/// Gracefully shutdown all tracked VMs on service termination signals.
///
/// Acquires the update lock with a short timeout so a slow in-flight
/// `/webhook` can't keep us racing forever (systemd would eventually
/// SIGKILL us and leave orphan qemu). When the lock can't be taken
/// in time, we still proceed and rely on the run-dir qemu reap below
/// for processes that escaped tracking (no host-wide `pkill`, so other
/// domains / other schedulers on the same host are not touched).
async fn shutdown_all_vms(state: &AppState) {
    tracing::info!("[shutdown] Initiating shutdown of all VMs");

    let _guard = state
        .try_lock_update(std::time::Duration::from_secs(10))
        .await;
    if _guard.is_none() {
        tracing::warn!(
            "[shutdown] timed out waiting for update lock; \
             proceeding with tracked machines and run-dir reap"
        );
    }

    let machines = state.take_all_machines().await;
    if machines.is_empty() {
        tracing::info!("[shutdown] No tracked VMs");
    } else {
        for (info, machine) in machines {
            tracing::info!(
                "[shutdown] Shutting down {} (domain={})",
                info.id,
                info.domain
            );
            match machine.shutdown().await {
                Ok(_) => tracing::info!("[shutdown] {} shut down OK", info.id),
                Err(e) => tracing::error!("[shutdown] {} failed: {}", info.id, e),
            }
        }
    }

    // Reap any qemu that escaped tracking — racing creates whose
    // KeepAliveMachine never made it into `state`, or processes left over
    // from a previous crashed run. Scoped to this process's qlean runs/
    // (via qemu.pid), not a host-wide pkill.
    vm_cleanup::reap_qemu_from_runs().await;

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

    // Cleanup residual qemu from previous runs (scoped to this XDG data tree),
    // then sweep on-disk run dirs. Unlike the old host-wide `pkill -f qemu`,
    // this only touches pids recorded under ~/.local/share/qlean/runs/ so
    // concurrent VMs owned by other domains/processes are not killed. qlean
    // only deletes those dirs from `Machine::drop`, which never runs on
    // SIGKILL/abort — leftover overlays are ~0.5–3 GB each.
    tracing::info!("[startup] Reaping stale qemu from qlean runs/");
    vm_cleanup::reap_qemu_from_runs().await;
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
        "[startup] Config loaded, default_image path: {}, max_vms: {:?}",
        config.read().await.default_image().image_path,
        config.read().await.max_vms()
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
        .route("/vms/{id}", axum::routing::get(handlers::vm_by_id_handler))
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
        .route(
            "/shutdown/all",
            axum::routing::post(handlers::shutdown_all_handler),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state.clone());

    // Start server (`LISTEN_ADDR` overrides the default for multi-process setups)
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    tracing::info!("[startup] Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Handle termination signals: stop all tracked VMs and the server
    let term_shutdown_state = state.clone();
    let term_shutdown_signal = async move {
        if let Some(()) = tokio::signal::unix::signal(SignalKind::terminate())
            .unwrap()
            .recv()
            .await
        {
            tracing::info!("[shutdown] Received SIGTERM");
            shutdown_all_vms(&term_shutdown_state).await;
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
            shutdown_all_vms(&quit_shutdown_state).await;
        }
    };

    // Handle Ctrl+C: stop all tracked VMs and the server
    let ctrl_c_shutdown_state = state.clone();
    let ctrl_c_signal = async move {
        match ctrl_c().await {
            Ok(()) => {
                tracing::info!("[shutdown] Received SIGINT (Ctrl+C)");
                shutdown_all_vms(&ctrl_c_shutdown_state).await;
            }
            Err(e) => tracing::error!("[shutdown] Ctrl+C handler error: {}", e),
        }
    };

    tracing::info!("[startup] Server running. Use /shutdown?domain=… to stop one VM");
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
