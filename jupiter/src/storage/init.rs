use std::{
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    time::Duration,
};

use common::{config::DbConfig, errors::MegaError};
#[cfg(feature = "migrate")]
use jupiter_migrate::apply_migrations;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tracing::log;
use url::Url;

use crate::utils::id_generator;

/// Create a database connection with failover logic.
///
/// This function attempts to connect to a database based on the provided configuration:
/// - If PostgreSQL is specified but unavailable, it automatically falls back to SQLite
/// - For local PostgreSQL connections, it first checks port reachability to avoid long timeouts
///
/// The failover logic works as follows:
/// 1. For PostgreSQL connections:
///    - If the host is local (localhost, 127.0.0.1, etc.), performs a quick port check (100ms timeout)
///    - If port is unreachable, immediately falls back to SQLite without waiting for a full connection timeout
///    - If port is reachable but connection fails, logs the error and falls back to SQLite
/// 2. For non-local PostgreSQL, attempts connection with normal timeouts (3 seconds)
///    - On failure, logs the error and falls back to SQLite
/// 3. For SQLite connections, connects directly without fallback
///
/// After successful connection, applies any pending database migrations.
///
/// This optimization helps avoid long waits when local PostgreSQL isn't running.
pub async fn database_connection(db_config: &DbConfig) -> DatabaseConnection {
    id_generator::set_up_options().unwrap();

    let conn = if db_config.db_type == "postgres" {
        if should_check_port_first(&db_config.db_url) {
            match check_local_postgres_and_connect(db_config).await {
                Ok(conn) => conn,
                Err(reason) => {
                    log::warn!(
                        "Falling back to SQLite for {}. Reason: {}",
                        db_config.db_url,
                        reason
                    );
                    sqlite_connection(db_config)
                        .await
                        .expect("Cannot connect to any database")
                }
            }
        } else {
            // online connect
            match postgres_connection(db_config).await {
                Ok(conn) => conn,
                Err(e) => {
                    log::error!("Failed to connect to postgres: {e}");
                    log::info!("Falling back to sqlite");
                    sqlite_connection(db_config)
                        .await
                        .expect("Cannot connect to any database")
                }
            }
        }
    } else {
        sqlite_connection(db_config).await.unwrap()
    };
    #[cfg(feature = "migrate")]
    apply_migrations(&conn, false)
        .await
        .expect("Failed to apply migrations");

    conn
}

fn should_check_port_first(db_url: &str) -> bool {
    if let Ok(url) = Url::parse(db_url)
        && let Some(host) = url.host_str()
    {
        return host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0";
    }
    false
}

/// Check local postgres port and try connecting
async fn check_local_postgres_and_connect(
    db_config: &DbConfig,
) -> Result<DatabaseConnection, String> {
    if !is_port_reachable(&db_config.db_url) {
        return Err("Local postgres port not reachable".to_string());
    }
    match postgres_connection(db_config).await {
        Ok(conn) => Ok(conn),
        Err(e) => Err(format!("Postgres connection failed: {}", e)),
    }
}

/// Check if any resolved address is reachable within 100ms
fn is_port_reachable(db_url: &str) -> bool {
    if let Ok(url) = Url::parse(db_url)
        && let (Some(host), Some(port)) = (url.host_str(), url.port())
    {
        if let Ok(addrs) = (host, port).to_socket_addrs() {
            for addr in addrs {
                if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
                    log::info!("Successfully connected to {}", addr);
                    return true;
                } else {
                    log::warn!("Failed to connect to {}", addr);
                }
            }
        } else {
            log::warn!("Failed to resolve host: {}", host);
        }
    }
    false
}

async fn postgres_connection(db_config: &DbConfig) -> Result<DatabaseConnection, MegaError> {
    let db_url = db_config.db_url.to_owned();
    log::info!("Connecting to database: {db_url}");

    let opt = setup_option(db_config);
    Database::connect(opt).await.map_err(|e| e.into())
}

async fn sqlite_connection(db_config: &DbConfig) -> Result<DatabaseConnection, MegaError> {
    if !Path::new(&db_config.db_path).exists() {
        eprintln!("Creating new sqlite database: {:?}", db_config.db_path);
        std::fs::create_dir_all(Path::new(&db_config.db_path).parent().unwrap())?;
        std::fs::File::create(&db_config.db_path)?;
    }
    let db_url = format!("sqlite://{}", db_config.db_path.to_string_lossy());
    log::info!("Connecting to database: {db_url}");

    let opt = setup_option(db_config);
    let conn = Database::connect(opt).await?;

    Ok(conn)
}

fn setup_option(db_config: &DbConfig) -> ConnectOptions {
    let mut opt = ConnectOptions::new(db_config.db_url.clone());
    opt.max_connections(db_config.max_connection)
        .min_connections(db_config.min_connection)
        .acquire_timeout(Duration::from_secs(db_config.acquire_timeout))
        .connect_timeout(Duration::from_secs(db_config.connect_timeout))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8))
        .sqlx_logging(db_config.sqlx_logging)
        .sqlx_logging_level(log::LevelFilter::Debug);
    opt
}

#[cfg(test)]
pub mod test {
    use super::*;

    /// Creates a test database connection for unit tests.
    pub fn test_local_db_address() {
        assert!(
            "postgres://mono:mono@localhost:5432/mono_test"
                .parse::<Url>()
                .is_ok()
        );

        // Test localhost variants - should return true
        assert!(should_check_port_first(
            "postgres://mono:mono@localhost:5432/mono_test"
        ));
        assert!(should_check_port_first(
            "postgres://mono:mono@127.0.0.1:5432/mono_test"
        ));
        assert!(should_check_port_first(
            "postgres://mono:mono@::1:5432/mono_test"
        ));
        assert!(should_check_port_first(
            "postgres://mono:mono@0.0.0.0:5432/mono_test"
        ));

        // Test remote addresses - should return false
        assert!(!should_check_port_first(
            "postgres://mono:mono@192.168.1.100:5432/mono_test"
        ));
        assert!(!should_check_port_first(
            "postgres://mono:mono@example.com:5432/mono_test"
        ));
        assert!(!should_check_port_first(
            "postgres://mono:mono@10.0.0.1:5432/mono_test"
        ));

        // Test invalid URLs - should return false
        assert!(!should_check_port_first("invalid_url"));
        assert!(!should_check_port_first(""));
    }
}
