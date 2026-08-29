use std::sync::Arc;

use jupiter::{
    redis::{ConnectionManager, claim_snowflake_worker, init_connection},
    utils::id_generator,
};

/// Main application context for the Mono application.
#[derive(Clone)]
pub struct AppContext {
    pub storage: jupiter::storage::Storage,
    pub vault: vault::integration::vault_core::VaultCore,
    pub config: Arc<common::config::Config>,
    pub connection: ConnectionManager,
}

impl AppContext {
    pub async fn new(config: common::config::Config) -> Self {
        let config = Arc::new(config);

        let connection = init_connection(&config.redis).await;
        if env_worker_id_is_set() {
            tracing::info!("MEGA_ID_GENERATOR_WORKER_ID set; skipping Redis snowflake slot claim");
        } else if let Some(id) = claim_snowflake_worker(&connection).await {
            id_generator::claim_worker_id(id);
        }

        let storage = jupiter::storage::Storage::new(config.clone())
            .await
            .expect("init monorepo storage err");

        let storage_for_vault = storage.clone();
        let vault = vault::integration::vault_core::VaultCore::new(storage_for_vault).await;

        storage
            .mono_service
            .init_monorepo(&config.monorepo)
            .await
            .expect("init monorepo failed");

        Self {
            storage,
            vault,
            config,
            connection,
        }
    }

    pub fn wrapped_context(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }
}

fn env_worker_id_is_set() -> bool {
    match std::env::var(id_generator::ENV_WORKER_ID) {
        Ok(raw) => raw
            .parse::<u32>()
            .ok()
            .is_some_and(|id| id <= id_generator::MAX_WORKER_ID),
        Err(_) => false,
    }
}
