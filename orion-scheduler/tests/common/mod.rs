#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    sync::Once,
    time::Duration,
};

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

pub const POSTGRES_CONTAINER: &str = "mega-demo-postgres";
pub const REDIS_CONTAINER: &str = "mega-demo-redis";
pub const DOCKER_COMPOSE_FILE: &str = "/tmp/docker-compose.yml";
pub const DOCKER_COMPOSE_HOST_PATH: &str = "docker/demo/docker-compose.demo.yml";

pub const MEGA_HOST: &str = "127.0.0.1";
pub const MEGA_PORT: u16 = 8000;
pub const POSTGRES_USER: &str = "mega";
pub const POSTGRES_PASSWORD: &str = "mega";
pub const POSTGRES_DB: &str = "mono";

pub const MYSQL_CONTAINER: &str = "mega-demo-mysql";

pub const CAMPSITE_API_CONTAINER: &str = "mega-demo-campsite-api";
pub const CAMPSITE_API_PORT: u16 = 8080;

pub const MEGA_STARTUP_WAIT_SECS: u64 = 5; // Wait time after starting Mega service
pub const DB_OP_WAIT_SECS: u64 = 2; // Wait time after database operations

// ECR mono image
pub const MEGA_ECR_IMAGE_DEFAULT: &str = "public.ecr.aws/m8q5m4u3/mega/mono-engine:latest-amd64";

fn get_mega_ecr_image() -> String {
    std::env::var("MEGA_ECR_IMAGE").unwrap_or_else(|_| MEGA_ECR_IMAGE_DEFAULT.to_string())
}

pub fn tracing_subscriber_init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
    });
}

/// Helper to run a command and check its exit status
pub async fn exec_check(vm: &mut qlean::Machine, cmd: &str) -> Result<String> {
    let result = vm.exec(cmd).await?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        anyhow::bail!(
            "Command '{}' failed with exit code {:?}\nstdout: {}\nstderr: {}",
            cmd,
            result.status.code(),
            stdout,
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).to_string())
}

/// Generic retry helper with success predicate
///
/// Repeatedly executes a command until it satisfies the predicate or max retries are reached.
pub async fn retry_until<F>(
    vm: &mut qlean::Machine,
    cmd: &str,
    success_predicate: F,
    service_name: &str,
    max_retries: u32,
    delay_secs: u64,
) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    let mut retries = 0;
    let mut last_error: Option<String> = None;
    let mut last_output: Option<String> = None;

    loop {
        match exec_check(vm, cmd).await {
            Ok(output) => {
                if success_predicate(&output) {
                    tracing::info!("{} is ready.", service_name);
                    return Ok(());
                }
                // Log non-matching successful output at debug level
                tracing::debug!(
                    "{} check attempt {}/{}: predicate not met, output: {}",
                    service_name,
                    retries + 1,
                    max_retries,
                    output.trim()
                );
                last_output = Some(output);
            }
            Err(e) => {
                // Log command failure at debug level
                tracing::debug!(
                    "{} check attempt {}/{} failed: {}",
                    service_name,
                    retries + 1,
                    max_retries,
                    e
                );
                last_error = Some(e.to_string());
            }
        }

        retries += 1;
        if retries >= max_retries {
            let mut msg = format!(
                "{} did not become ready after {} seconds",
                service_name,
                (max_retries as u64) * delay_secs
            );
            if let Some(err) = &last_error {
                msg.push_str(&format!("\nLast error: {}", err));
            }
            if let Some(output) = &last_output {
                msg.push_str(&format!("\nLast output: {}", output.trim()));
            }
            anyhow::bail!(msg);
        }

        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
    }
}

/// Wait for Mega API to be ready by polling the status endpoint
async fn wait_for_mega_service(vm: &mut qlean::Machine, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    let status_url = format!("http://{}:{}/api/v1/status", MEGA_HOST, MEGA_PORT);

    tracing::info!("Waiting for Mega service at {}...", status_url);

    loop {
        let result = vm
            .exec(&format!(
                "curl -sf -o /dev/null -w \"%{{http_code}}\" \"{}\"",
                status_url
            ))
            .await?;

        let status_code = String::from_utf8_lossy(&result.stdout).trim().to_string();
        tracing::debug!("Mega service check returned status: {}", status_code);

        if status_code == "200" {
            tracing::info!("Mega service is ready (status: {})", status_code);
            return Ok(());
        }

        if start.elapsed() > timeout {
            let log_output =
                exec_check(vm, "cat /tmp/mega.log 2>/dev/null || echo 'No log file'").await?;
            tracing::error!("Mega service logs:\n{}", log_output);
            anyhow::bail!(
                "Timeout waiting for Mega service at {} (last status: {})",
                status_url,
                status_code
            );
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Install Docker in the VM
pub async fn install_docker(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Installing Docker in VM...");

    // Update package list
    exec_check(vm, "apt-get update -qq").await?;

    // Install prerequisites
    exec_check(
        vm,
        "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
            ca-certificates \
            curl \
            gnupg \
            lsb-release",
    )
    .await?;

    // Add Docker's official GPG key
    exec_check(vm, "install -m 0755 -d /etc/apt/keyrings").await?;

    exec_check(
        vm,
        "curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg",
    )
    .await?;

    exec_check(vm, "chmod a+r /etc/apt/keyrings/docker.gpg").await?;

    // Set up Docker repository
    exec_check(
        vm,
        "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
            https://download.docker.com/linux/debian $(. /etc/os-release && echo $VERSION_CODENAME) stable\" \
            > /etc/apt/sources.list.d/docker.list",
    )
    .await?;

    // Update package list again
    exec_check(vm, "apt-get update -qq").await?;

    // Install Docker Engine
    exec_check(
        vm,
        "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
            docker-ce \
            docker-ce-cli \
            containerd.io \
            docker-compose-plugin",
    )
    .await?;

    // Start Docker service
    exec_check(vm, "service docker start").await?;

    // Verify Docker is running
    exec_check(vm, "docker info > /dev/null").await?;

    tracing::info!("Docker installed and started successfully.");

    // Upload docker-compose.demo.yml to VM
    tracing::info!("Uploading docker-compose.demo.yml to VM...");
    let host_compose_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(DOCKER_COMPOSE_HOST_PATH);

    // Try to read compose file from host
    let content = std::fs::read_to_string(&host_compose_path).with_context(|| {
        format!(
            "Failed to read docker-compose.demo.yml from {}",
            host_compose_path.display()
        )
    })?;

    vm.write(Path::new(DOCKER_COMPOSE_FILE), content.as_bytes())
        .await?;

    tracing::info!(
        "Uploaded docker-compose.demo.yml from {} to {}",
        host_compose_path.display(),
        DOCKER_COMPOSE_FILE
    );

    Ok(())
}

/// Setup PostgreSQL using Docker in VM
pub async fn setup_postgres(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Setting up PostgreSQL...");

    // Start PostgreSQL container using uploaded compose file
    // Override environment variables to use test credentials instead of compose defaults
    tracing::info!("Starting PostgreSQL container with test credentials...");
    exec_check(
        vm,
        &format!(
            "POSTGRES_USER={} POSTGRES_PASSWORD={} POSTGRES_DB_MONO={} docker compose -f {} up -d postgres",
            POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB, DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    // Wait for PostgreSQL to be ready using retry helper
    tracing::info!("Waiting for PostgreSQL to be ready...");
    retry_until(
        vm,
        &format!(
            "docker exec {} pg_isready -U {}",
            POSTGRES_CONTAINER, POSTGRES_USER
        ),
        |output| output.contains("accepting connections"),
        "PostgreSQL",
        30,
        2,
    )
    .await?;

    // Grant schema permissions for PostgreSQL 15+
    tracing::info!("Configuring PostgreSQL permissions...");
    exec_check(
        vm,
        &format!(
            "docker exec {} psql -U {} -d {} -c \"GRANT ALL ON SCHEMA public TO {};\"",
            POSTGRES_CONTAINER, POSTGRES_USER, POSTGRES_DB, POSTGRES_USER
        ),
    )
    .await?;

    exec_check(
        vm,
        &format!(
            "docker exec {} psql -U {} -d {} -c \"GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO {};\"",
            POSTGRES_CONTAINER, POSTGRES_USER, POSTGRES_DB, POSTGRES_USER
        ),
    )
    .await?;

    exec_check(
        vm,
        &format!(
            "docker exec {} psql -U {} -d {} -c \"ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO {};\"",
            POSTGRES_CONTAINER, POSTGRES_USER, POSTGRES_DB, POSTGRES_USER
        ),
    )
    .await?;

    tokio::time::sleep(Duration::from_secs(DB_OP_WAIT_SECS)).await;

    tracing::info!("PostgreSQL setup complete.");
    Ok(())
}

/// Setup Redis using Docker in VM
pub async fn setup_redis(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Setting up Redis...");

    // Start Redis container using the uploaded compose file
    tracing::info!("Starting Redis container...");
    exec_check(
        vm,
        &format!("docker compose -f {} up -d redis", DOCKER_COMPOSE_FILE),
    )
    .await?;

    // Wait for Redis to be ready using retry helper
    tracing::info!("Waiting for Redis to be ready...");
    retry_until(
        vm,
        &format!("docker exec {} redis-cli ping", REDIS_CONTAINER),
        |output| output.trim() == "PONG",
        "Redis",
        15,
        2,
    )
    .await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::info!("Redis setup complete.");
    Ok(())
}

/// Setup and start Mega service
pub async fn setup_mega_service(vm: &mut qlean::Machine) -> Result<()> {
    // Clean up any existing directories from previous test runs
    tracing::info!("Cleaning up existing Mega directories from previous runs...");
    exec_check(
        vm,
        "rm -rf /tmp/mega /tmp/mono /tmp/repo_* 2>/dev/null || true",
    )
    .await?;

    tracing::info!("Creating Mega directories...");
    exec_check(vm, "mkdir -p /tmp/mega/cache").await?;
    exec_check(vm, "mkdir -p /tmp/mega/logs").await?;
    exec_check(vm, "mkdir -p /tmp/mega/import").await?;
    exec_check(vm, "mkdir -p /tmp/mega/lfs").await?;
    exec_check(vm, "mkdir -p /tmp/mega/objects").await?;
    exec_check(vm, "mkdir -p /root/.local/share").await?;
    exec_check(vm, "mkdir -p /root/.local/share/mega/etc").await?;

    tracing::info!("Pulling mono image from ECR...");
    let ecr_image = get_mega_ecr_image();
    exec_check(vm, &format!("docker pull {}", ecr_image)).await?;

    tracing::info!("Extracting mono binary from ECR image...");
    exec_check(
        vm,
        &format!(
            "docker run --rm -v /usr/local/bin:/output {} cp /usr/local/bin/mono /output/",
            ecr_image
        ),
    )
    .await?;
    exec_check(vm, "chmod +x /usr/local/bin/mono").await?;

    // Install curl, jq, git (needed for test execution)
    exec_check(
        vm,
        "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl jq git",
    )
    .await?;

    let config_content = format!(
        r#"base_dir = "/tmp/mega"

[log]
level = "info"
print_std = true
with_ansi = false

[database]
db_type = "postgres"
db_path = "/tmp/mega/mega.db"
db_url = "postgres://{}:{}@127.0.0.1:5432/{}"
max_connection = 16
min_connection = 8
acquire_timeout = 3
connect_timeout = 3
sqlx_logging = false

[monorepo]
import_dir = "/tmp/mega/import"
admin = ["admin"]
root_dirs = ["third-party", "project", "doc", "release"]

[build]
enable_build = true
orion_server = "http://localhost:8004"

[pack]
pack_decode_mem_size = "4G"
pack_decode_disk_size = "20%"
pack_decode_cache_path = "/tmp/mega/cache"
clean_cache_after_decode = true
channel_message_size = 1000000

[lfs]

[lfs.ssh]
http_url = "http://localhost:8000"

[lfs.local]
lfs_file_path = "/tmp/mega/lfs"

[object_storage]
storage_type = "local"

[object_storage.s3]
region = "us-east-1"
bucket = "mega"
access_key_id = ""
secret_access_key = ""
endpoint_url = ""

[object_storage.gcs]
bucket = "gitmega"

[object_storage.local]
root_dir = "/tmp/mega/objects"

[redis]
url = "redis://127.0.0.1:6379"
"#,
        POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB
    );

    vm.write(
        std::path::Path::new("/root/.local/share/mega/etc/config.toml"),
        config_content.as_bytes(),
    )
    .await?;

    tracing::info!("Starting Mega service in background...");
    exec_check(vm, "nohup mono service http > /tmp/mega.log 2>&1 &").await?;

    tokio::time::sleep(Duration::from_secs(MEGA_STARTUP_WAIT_SECS)).await;

    let ps_output = exec_check(vm, "ps aux | grep '[m]ono' || true").await?;
    tracing::debug!("Mega process status: {}", ps_output);

    wait_for_mega_service(vm, 60).await?;

    tracing::info!("Mega service is ready.");
    Ok(())
}

pub async fn setup_mysql(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Setting up MySQL for Campsite API...");

    exec_check(
        vm,
        &format!(
            "MYSQL_ROOT_PASSWORD=mysqladmin MYSQL_DATABASE=campsite_api_demo docker compose -f {} up -d mysql",
            DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    retry_until(
        vm,
        &format!(
            "docker exec {} mysqladmin ping -h localhost -u root -pmysqladmin",
            MYSQL_CONTAINER
        ),
        |output| output.contains("alive"),
        "MySQL",
        45,
        2,
    )
    .await?;

    tracing::info!("MySQL is ready");
    Ok(())
}

pub async fn setup_campsite_api(vm: &mut qlean::Machine) -> Result<()> {
    tracing::info!("Setting up Campsite API using Docker...");

    tracing::info!("Pulling Campsite API image...");
    let ecr_image = get_mega_ecr_image();
    exec_check(vm, &format!("docker pull {}", ecr_image)).await?;
    tracing::info!("Image pulled successfully");

    tracing::info!("Starting Campsite API container...");
    exec_check(
        vm,
        &format!(
            "docker compose -f {} up -d campsite_api",
            DOCKER_COMPOSE_FILE
        ),
    )
    .await?;

    tokio::time::sleep(Duration::from_secs(3)).await;

    let container_status = vm
        .exec(&format!(
            "docker inspect -f '{{{{.State.Status}}}}' {} 2>/dev/null || echo 'unknown'",
            CAMPSITE_API_CONTAINER
        ))
        .await?;
    let status = String::from_utf8_lossy(&container_status.stdout);
    tracing::info!("Container status: {}", status.trim());

    if status.trim() != "running" {
        let logs = vm
            .exec(&format!(
                "docker logs {} 2>&1 | tail -30",
                CAMPSITE_API_CONTAINER
            ))
            .await?;
        tracing::error!(
            "Container failed to start. Logs:\n{}",
            String::from_utf8_lossy(&logs.stdout)
        );
        anyhow::bail!("Campsite API container is not running");
    }

    tracing::info!("Waiting for Campsite API to be ready...");

    let start_time = std::time::Instant::now();
    let check_interval = Duration::from_secs(2);
    let log_interval = Duration::from_secs(30);
    let mut last_log_time = start_time;

    loop {
        let check_cmd = format!(
            "curl -s -o /dev/null -w '%{{http_code}}' http://127.0.0.1:{}/health 2>/dev/null || echo 'not_ready'",
            CAMPSITE_API_PORT
        );

        match exec_check(vm, &check_cmd).await {
            Ok(output) if output.trim() == "200" => {
                let elapsed = start_time.elapsed().as_secs();
                tracing::info!("Campsite API is ready after {} seconds", elapsed);
                break;
            }
            _ => {
                if last_log_time.elapsed() >= log_interval {
                    let logs_result = vm
                        .exec(&format!(
                            "docker logs --tail 5 {} 2>&1 || echo 'No logs yet'",
                            CAMPSITE_API_CONTAINER
                        ))
                        .await;
                    if let Ok(logs) = logs_result {
                        let stdout = String::from_utf8_lossy(&logs.stdout);
                        if !stdout.trim().is_empty() && stdout != "No logs yet" {
                            tracing::info!(
                                "Campsite API is still starting... Last log:\n{}",
                                stdout.trim()
                            );
                        }
                    }
                    last_log_time = std::time::Instant::now();
                }
            }
        }

        if start_time.elapsed() > Duration::from_secs(900) {
            anyhow::bail!("Campsite API failed to start within 15 minutes");
        }

        tokio::time::sleep(check_interval).await;
    }

    tracing::info!("Campsite API setup complete.");
    Ok(())
}
