// Orion worker client modules
mod antares;
mod api;
mod buck_controller;
mod disk;
pub mod repo;
mod util;
mod ws;

use std::io::{LineWriter, stderr};

use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    // Load environment variables from .env file before initializing logging so
    // log filters can be configured via RUST_LOG in local dev and deployments.
    dotenvy::dotenv().ok();

    // LineWriter flushes on every newline so redirected stderr (run.sh → orion.log)
    // stays live for remote tail/SSE consumers instead of block-buffering.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,rfuse3::raw::session=error")),
        )
        .with_target(true)
        .with_writer(|| LineWriter::new(stderr()))
        .init();

    // Configure WebSocket server address
    let server_addr =
        std::env::var("SERVER_WS").unwrap_or_else(|_| "ws://127.0.0.1:8004/ws".to_string());

    // Configure worker identification
    let worker_id = std::env::var("ORION_WORKER_ID").unwrap_or_else(|_| {
        tracing::warn!("ORION_WORKER_ID not set, generating a random worker ID for this session.");
        // Generate time-ordered UUID for better traceability
        Uuid::new_v4().to_string()
    });

    tracing::info!("Starting orion worker...");
    tracing::info!("  Worker ID: {}", worker_id);
    tracing::info!("  Connecting to server at: {}", server_addr);

    if let Err(err) = antares::warmup_dicfuse().await {
        tracing::warn!("Antares startup warmup failed: {}", err);
    }

    // Start WebSocket client with persistent connection
    ws::run_client(server_addr, worker_id).await;
}
