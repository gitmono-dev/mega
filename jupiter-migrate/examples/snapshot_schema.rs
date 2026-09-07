//! Prepare a NEW, local SQLite database from the real migrations for codegen.
//! Usage: cargo run -p jupiter-migrate --example snapshot_schema -- /tmp/new.db
//! Existing files are rejected so a developer cannot overwrite a database.

use std::{fs::OpenOptions, path::PathBuf};

use sea_orm::{ConnectOptions, Database};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected a new SQLite file path")?,
    );
    if !path.is_absolute() {
        return Err("expected an absolute SQLite file path".into());
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let url = format!(
        "sqlite://{}",
        path.to_str().ok_or("SQLite path must be UTF-8")?
    );
    let db = Database::connect(ConnectOptions::new(url)).await?;
    jupiter_migrate::apply_migrations(&db, false).await?;
    db.close().await?;
    println!("{}", path.display());
    Ok(())
}
