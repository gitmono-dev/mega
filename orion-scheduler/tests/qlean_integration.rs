#![cfg(target_os = "linux")]

//! Qlean Integration Tests - Simplified Version
//!
//! This version uses docker-compose.demo.yml for ALL services,
//! only compiling Orion client on the VM.

mod common;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use common::*;
use qlean::{Distro, GuestArch, Image, ImageConfig, MachineConfig, with_machine};

const TEST_USER: &str = "mega";
const TEST_TOKEN: &str = "mega";
const DOCKER_COMPOSE_FILE: &str = "/tmp/docker-compose.demo.yml";

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

/// Upload entire mega project to VM
async fn upload_required_files(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Uploading mega project to VM...");

    let workspace_root =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));
    let mega_root = workspace_root
        .parent()
        .expect("Failed to get parent")
        .to_path_buf();

    tracing::info!("Mega project root: {:?}", mega_root);

    // Clean up
    vm.exec("rm -rf /tmp/mega").await?;
    vm.exec("mkdir -p /tmp/mega").await?;

    let mega_root_str = mega_root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid path: {:?}", mega_root))?;

    // Create tarball
    let tar_cmd = format!(
        "cd {} && tar --exclude='target' --exclude='node_modules' --exclude='.next' \
         --exclude='dist' --exclude='build' --exclude='.git' --exclude='__pycache__' \
         --exclude='*.pyc' --exclude='.venv' -czf /tmp/mega-upload.tar.gz .",
        mega_root_str
    );

    let tar_result = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&tar_cmd)
        .output()
        .await?;

    if !tar_result.status.success() {
        anyhow::bail!(
            "Failed to create tarball: {}",
            String::from_utf8_lossy(&tar_result.stderr)
        );
    }

    // Upload and extract
    vm.upload(
        Path::new("/tmp/mega-upload.tar.gz"),
        Path::new("/tmp/mega-upload.tar.gz"),
    )
    .await?;

    vm.exec("cd /tmp/mega && tar -xzf /tmp/mega-upload.tar.gz && rm /tmp/mega-upload.tar.gz")
        .await?;

    let _ = tokio::process::Command::new("rm")
        .arg("/tmp/mega-upload.tar.gz")
        .output()
        .await;

    tracing::info!("✓ Project uploaded");
    Ok(())
}

/// Install system dependencies
async fn install_dependencies(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Installing system dependencies...");

    // Update apt
    exec_check(vm, "apt-get update -qq").await?;

    // Install Docker
    if vm.exec("which docker").await?.status.success() {
        tracing::info!("Docker already installed");
    } else {
        tracing::info!("Installing Docker...");
        exec_check(
            vm,
            "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ca-certificates curl gnupg",
        )
        .await?;
        exec_check(vm, "install -m 0755 -d /etc/apt/keyrings").await?;
        exec_check(
            vm,
            "curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg"
        ).await?;
        exec_check(
            vm,
            "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian $(. /etc/os-release && echo $VERSION_CODENAME) stable\" > /etc/apt/sources.list.d/docker.list"
        ).await?;
        exec_check(vm, "apt-get update -qq").await?;
        exec_check(
            vm,
            "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin"
        ).await?;

        // Configure Docker mirrors
        let daemon_json = r#"{
  "registry-mirrors": [
    "https://docker.m.daocloud.io",
    "https://docker.1panel.live",
    "https://hub.rat.dev"
  ],
  "max-concurrent-downloads": 10
}"#;
        vm.write(Path::new("/etc/docker/daemon.json"), daemon_json.as_bytes())
            .await?;
        exec_check(vm, "service docker restart").await?;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    // Install build tools
    exec_check(
        vm,
        "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config \
         libssl-dev libclang-dev cmake curl git jq fuse3 zstd",
    )
    .await?;

    // Install Rust
    if !vm.exec("which cargo").await?.status.success() {
        tracing::info!("Installing Rust...");
        exec_check(
            vm,
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
        )
        .await?;
    }

    // Install Buck2 - must match version used by orion (see .devcontainer/setup.sh)
    if !vm.exec("which buck2").await?.status.success() {
        tracing::info!("Installing Buck2...");
        exec_check(
            vm,
            "curl -fsSL https://github.com/facebook/buck2/releases/download/2026-02-01/buck2-x86_64-unknown-linux-musl.zst \
             -o /tmp/buck2.zst && \
             zstd -d /tmp/buck2.zst -o /usr/local/bin/buck2 && \
             rm /tmp/buck2.zst && \
             chmod +x /usr/local/bin/buck2",
        )
        .await?;
    }

    tracing::info!("✓ Dependencies installed");
    Ok(())
}

/// Copy docker-compose file to expected location
async fn prepare_docker_compose(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Preparing docker-compose file...");
    exec_check(
        vm,
        &format!(
            "cp /tmp/mega/docker/demo/docker-compose.demo.yml {}",
            DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    // Copy required scripts
    exec_check(
        vm,
        "cp /tmp/mega/docker/demo/start-mono-wrapper.sh /tmp/start-mono-wrapper.sh && chmod +x /tmp/start-mono-wrapper.sh"
    ).await?;

    exec_check(
        vm,
        "cp /tmp/mega/docker/demo/init-rustfs-bucket.sh /tmp/init-rustfs-bucket.sh && chmod +x /tmp/init-rustfs-bucket.sh"
    ).await?;

    // Update paths in docker-compose.yml to use /tmp
    exec_check(
        vm,
        &format!(
            "sed -i 's|./start-mono-wrapper.sh|/tmp/start-mono-wrapper.sh|g' {}",
            DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    exec_check(
        vm,
        &format!(
            "sed -i 's|./init-rustfs-bucket.sh|/tmp/init-rustfs-bucket.sh|g' {}",
            DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    exec_check(vm, "cp /tmp/mega/docker/demo/.env.example /tmp/.env").await?;

    // Override MEGA_MONOREPO__ADMIN if ADMIN_USER environment variable is set
    if let Ok(admin_user) = std::env::var("ADMIN_USER") {
        if admin_user.trim().is_empty() {
            tracing::info!("ADMIN_USER is empty, MEGA_MONOREPO__ADMIN will remain empty");
        } else {
            tracing::info!(
                "Setting MEGA_MONOREPO__ADMIN from ADMIN_USER: {}",
                admin_user
            );
            // Use sed to replace the MEGA_MONOREPO__ADMIN line in .env
            // Using | as delimiter to avoid conflicts with / in values
            exec_check(
                vm,
                &format!(
                    "sed -i 's|^MEGA_MONOREPO__ADMIN=.*|MEGA_MONOREPO__ADMIN={}|' /tmp/.env",
                    admin_user.trim()
                ),
            )
            .await?;
        }
    } else {
        // Default to empty string if ADMIN_USER is not set
        tracing::info!("ADMIN_USER not set, clearing MEGA_MONOREPO__ADMIN to empty");
        exec_check(
            vm,
            "sed -i 's|^MEGA_MONOREPO__ADMIN=.*|MEGA_MONOREPO__ADMIN=|' /tmp/.env",
        )
        .await?;
    }

    tracing::info!("✓ Docker compose prepared");
    Ok(())
}

/// Start all Docker services and wait for health
async fn start_docker_services(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Starting Docker services...");

    // Pull images first (to show progress and use mirrors)
    tracing::info!("Pulling Docker images...");
    exec_check(
        vm,
        &format!("docker compose -f {} pull", DOCKER_COMPOSE_FILE),
    )
    .await?;

    // Start all services
    tracing::info!("Starting all services...");
    exec_check(
        vm,
        &format!("docker compose -f {} up -d", DOCKER_COMPOSE_FILE),
    )
    .await?;

    // Wait for key services to be healthy
    tracing::info!("Waiting for services to be healthy...");

    let services = [
        ("postgres", 60),
        ("redis", 30),
        ("mysql", 60),
        ("rustfs", 60),
        ("init-rustfs-bucket", 30),
        ("mega", 120),
        ("campsite_api", 180),
        ("orion_server", 90),
        ("mega_ui", 180),
    ];

    for (service, timeout) in services.iter() {
        tracing::info!("  Waiting for {}...", service);
        wait_for_service_health(vm, service, *timeout).await?;
    }

    tracing::info!("✓ All Docker services healthy");
    Ok(())
}

/// Wait for a specific service to be healthy
async fn wait_for_service_health(
    vm: &mut qlean::Machine,
    service: &str,
    timeout_secs: u64,
) -> Result<()> {
    let start = std::time::Instant::now();
    let interval = Duration::from_secs(5);

    // Special handling for init containers
    let is_init_container = service.starts_with("init-");

    while start.elapsed().as_secs() < timeout_secs {
        let result = vm
            .exec(&format!(
                "docker compose -f {} ps -a {}",
                DOCKER_COMPOSE_FILE, service
            ))
            .await?;

        let output = String::from_utf8_lossy(&result.stdout);

        // Debug: show actual output for init containers
        if is_init_container {
            tracing::debug!("Init container '{}' status output:\n{}", service, output);
        }

        // For init containers: check if exited successfully (exit code 0)
        if is_init_container {
            // Check if successfully completed (exit code 0)
            if output.contains("exited (0)")
                || output.contains("Exited (0)")
                || output.contains("Exit 0")
            {
                tracing::info!("    ✓ {} completed successfully", service);
                return Ok(());
            }

            // Check if failed (exited with non-zero code)
            if output.contains("exited")
                && !output.contains("(0)")
                && !output.contains("Exit 0")
                && !output.contains("Exited (0)")
            {
                let logs = vm
                    .exec(&format!(
                        "docker compose -f {} logs --tail 50 {}",
                        DOCKER_COMPOSE_FILE, service
                    ))
                    .await;
                if let Ok(logs) = logs {
                    tracing::error!(
                        "Init container '{}' failed:\n{}",
                        service,
                        String::from_utf8_lossy(&logs.stdout)
                    );
                }
                anyhow::bail!("Init container '{}' exited with non-zero code", service);
            }
        } else {
            // For regular services: check if healthy or running
            if output.contains("healthy")
                || (output.contains(service)
                    && output.contains("running")
                    && !output.contains("unhealthy"))
            {
                tracing::info!("    ✓ {} ready", service);
                return Ok(());
            }
        }

        tokio::time::sleep(interval).await;
    }

    // Show logs on failure
    let logs = vm
        .exec(&format!(
            "docker compose -f {} logs --tail 50 {}",
            DOCKER_COMPOSE_FILE, service
        ))
        .await;

    if let Ok(logs) = logs {
        tracing::error!(
            "Service '{}' logs:\n{}",
            service,
            String::from_utf8_lossy(&logs.stdout)
        );
    }

    anyhow::bail!(
        "Service '{}' did not become healthy within {}s",
        service,
        timeout_secs
    )
}

/// Setup test users
/// Setup Scorpio directories and config
async fn setup_scorpio(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Setting up Scorpio directories and config...");

    // Create all required directories
    exec_check(
        vm,
        "mkdir -p /tmp/megadir/store \
                  /tmp/megadir/antares/upper \
                  /tmp/megadir/antares/cl \
                  /tmp/megadir/antares/mnt \
                  /tmp/megadir/mount",
    )
    .await?;

    // Write a complete scorpio.toml pointing to local Mega service
    // Must include 'config_file' field (required by scorpiofs library)
    let scorpio_config = r#"# Scorpio config for Qlean integration test
# Points to local Mega service running in Docker

base_url = "http://localhost:8000"
lfs_url = "http://localhost:8000"

store_path = "/tmp/megadir/store"
workspace = "/tmp/megadir/mount"
config_file = "config.toml"

git_author = "MEGA"
git_email = "admin@mega.org"

dicfuse_readable = "true"
load_dir_depth = "3"
fetch_file_thread = "10"
dicfuse_import_concurrency = "4"
dicfuse_dir_sync_ttl_secs = "5"
dicfuse_reply_ttl_secs = "2"
dicfuse_fetch_dir_timeout_secs = "10"
dicfuse_connect_timeout_secs = "3"
dicfuse_fetch_dir_max_retries = "3"
dicfuse_stat_mode = "accurate"
dicfuse_open_buff_max_bytes = "268435456"
dicfuse_open_buff_max_files = "4096"

antares_load_dir_depth = "3"
antares_dicfuse_stat_mode = "fast"
antares_dicfuse_open_buff_max_bytes = "67108864"
antares_dicfuse_open_buff_max_files = "1024"
antares_dicfuse_dir_sync_ttl_secs = "120"
antares_dicfuse_reply_ttl_secs = "60"
antares_upper_root = "/tmp/megadir/antares/upper"
antares_cl_root = "/tmp/megadir/antares/cl"
antares_mount_root = "/tmp/megadir/antares/mnt"
antares_state_file = "/tmp/megadir/antares/state.toml"
"#;

    vm.write(
        std::path::Path::new("/tmp/scorpio.toml"),
        scorpio_config.as_bytes(),
    )
    .await?;

    tracing::info!("✓ Scorpio configured at /tmp/scorpio.toml");
    Ok(())
}

/// Compile Orion client
async fn compile_orion_client(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Compiling Orion client...");

    let build_cmd = "source $HOME/.cargo/env && cd /tmp/mega && cargo build --package orion";

    let result = vm.exec(build_cmd).await?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        anyhow::bail!("Orion compilation failed:\n{}", stderr);
    }

    tracing::info!("✓ Orion client compiled");
    Ok(())
}

/// Start Orion client
async fn start_orion_client(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Starting Orion client...");

    // Create mount directory (no cleanup needed - fresh VM)
    exec_check(vm, "mkdir -p /tmp/megadir/mount").await?;

    // Start Orion with environment variables directly passed to the process
    // This ensures child processes (Buck2) inherit BUCK2_DAEMON_DIR
    let start_cmd = r#"
        mkdir -p /tmp/buck2-daemon && \
        cd /tmp/mega && \
        SERVER_WS="ws://localhost:8004/ws" \
        MEGA_BASE_URL="http://localhost:8000" \
        MEGA_LFS_URL="http://localhost:8000" \
        SCORPIO_CONFIG="/tmp/scorpio.toml" \
        BUCK_PROJECT_ROOT="/tmp/megadir/mount" \
        RUST_LOG="info" \
        BUCK2_DAEMON_DIR="/tmp/buck2-daemon" \
        nohup ./target/debug/orion > /tmp/orion.log 2>&1 &
    "#;

    vm.exec(start_cmd).await?;
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Verify
    let result = vm
        .exec("pgrep -f 'target/.*orion' || echo 'NOT_RUNNING'")
        .await?;
    let output = String::from_utf8_lossy(&result.stdout).trim().to_string();

    if output == "NOT_RUNNING" {
        let logs = vm.exec("cat /tmp/orion.log").await?;
        anyhow::bail!(
            "Orion failed to start:\n{}",
            String::from_utf8_lossy(&logs.stdout)
        );
    }

    tracing::info!("✓ Orion client started (PID: {})", output);

    // Check logs for connection
    tokio::time::sleep(Duration::from_secs(3)).await;
    let logs = vm.exec("tail -30 /tmp/orion.log").await?;
    tracing::debug!("Orion logs:\n{}", String::from_utf8_lossy(&logs.stdout));

    Ok(())
}

// ====================================================================================
// Build Testing Functions (API-based E2E test)
// ====================================================================================

/// Initialize Git monorepo by cloning from Mega service
async fn init_monorepo(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Initializing Git monorepo...");

    // Configure git
    exec_check(vm, "git config --global user.name 'Test User'").await?;
    exec_check(vm, "git config --global user.email 'test@example.com'").await?;

    // Clean up and clone
    exec_check(vm, "rm -rf /tmp/buck2-project").await?;

    let clone_url = format!("http://{}:{}@127.0.0.1:8000/.git", TEST_USER, TEST_TOKEN);

    tracing::info!("Cloning monorepo from Mega...");
    exec_check(vm, &format!("git clone {} /tmp/buck2-project", clone_url)).await?;

    // Create initial commit if repo is empty
    vm.write(
        Path::new("/tmp/buck2-project/root.txt"),
        b"Initial mono file",
    )
    .await?;

    exec_check(
        vm,
        "cd /tmp/buck2-project && git add . && git commit -m 'Initial commit'",
    )
    .await?;

    exec_check(vm, "cd /tmp/buck2-project && git push").await?;

    tracing::info!("✓ Git monorepo initialized");
    Ok(())
}

/// Main setup function
pub async fn setup_mega_orion_environment(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("====================================================================");
    tracing::info!("  Mega + Orion Integration Test Environment Setup");
    tracing::info!("====================================================================\n");

    tracing::info!("[1/7] Uploading project files...");
    upload_required_files(vm).await?;

    tracing::info!("\n[2/7] Installing dependencies...");
    install_dependencies(vm).await?;

    tracing::info!("\n[3/7] Preparing Docker Compose...");
    prepare_docker_compose(vm).await?;

    tracing::info!("\n[4/7] Starting Docker services...");
    start_docker_services(vm).await?;

    tracing::info!("\n[5/7] Setting up Scorpio...");
    setup_scorpio(vm).await?;

    tracing::info!("\n[6/7] Compiling Orion client...");
    compile_orion_client(vm).await?;

    tracing::info!("\n[7/7] Starting Orion client...");
    start_orion_client(vm).await?;

    tracing::info!("\n====================================================================");
    tracing::info!("  ✓ Environment Setup Complete!");
    tracing::info!("====================================================================");
    Ok(())
}

// ==================== TESTS ====================

#[tokio::test]
#[ignore = "Interactive development environment"]
async fn test_qlean_dev_environment() -> Result<()> {
    init_tracing();

    let image = Image::new(
        ImageConfig::default()
            .with_distro(Distro::Debian)
            .with_arch(GuestArch::Amd64),
    )
    .await?;

    with_machine(
        &image,
        &MachineConfig {
            core: 4,
            mem: 8192,
            disk: Some(20),
            clear: true,
            ..Default::default()
        },
        |vm| {
            Box::pin(async {
                setup_mega_orion_environment(vm).await?;

                // Initialize repository to show in UI
                tracing::info!("Initializing monorepo for UI display...");
                init_monorepo(vm).await?;

                if std::env::var("QLEAN_KEEP_ALIVE").is_ok() {
                    let vm_ip = vm.get_ip().await.unwrap_or_else(|_| "unknown".to_string());
                    let admin_user = std::env::var("ADMIN_USER").unwrap_or_default();

                    println!("\n╔══════════════════════════════════════════════════════════╗");
                    println!("║  Mega + Orion Environment Ready                          ║");
                    println!("╠══════════════════════════════════════════════════════════╣");
                    println!("║  VM IP: {:<48} ║", vm_ip);
                    if !admin_user.trim().is_empty() {
                        println!("║  Admin User: {:<46}║", admin_user.trim());
                    } else {
                        println!("║  Admin User: (not configured)                            ║");
                    }
                    println!("╠══════════════════════════════════════════════════════════╣");
                    println!("║  Add to /etc/hosts:                                      ║");
                    println!("║  {} app.gitmono.local                       ║", vm_ip);
                    println!("║  {} auth.gitmono.local                      ║", vm_ip);
                    println!("║  {} api.gitmono.local                       ║", vm_ip);
                    println!("║  {} git.gitmono.local                       ║", vm_ip);
                    println!("║  {} orion.gitmono.local                     ║", vm_ip);
                    println!("╠══════════════════════════════════════════════════════════╣");
                    println!("║  UI: http://app.gitmono.local                            ║");
                    println!("╚══════════════════════════════════════════════════════════╝\n");

                    println!("Press Ctrl+C to shutdown...");
                    tokio::signal::ctrl_c().await?;
                }

                Ok(())
            })
        },
    )
    .await
}
