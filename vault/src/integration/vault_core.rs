use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use common::errors::MegaError;
use jupiter::storage::Storage;
use libvault::{RustyVault, logical::Response, storage::Backend};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::log;

use crate::integration::jupiter_backend::JupiterBackend;

const CORE_KEY_FILE: &str = "core_key.json"; // where the core key is stored, like `root_token`

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoreKey {
    secret_shares: Vec<Vec<u8>>,
    root_token: String,
}

#[derive(Clone)]
pub struct VaultCore {
    rvault: Arc<RustyVault>,
    key: Arc<CoreKey>,
}

/// This is a tool trait that provides methods to interact with the vault core.
/// Commonly you don't need to implement this trait, but use `VaultCore` directly.
/// It provides methods to read, write, and delete secrets in the vault.
#[async_trait]
pub trait VaultCoreInterface {
    fn token(&self) -> &str;
    async fn read_api(&self, path: impl AsRef<str> + Send) -> Result<Option<Response>, MegaError>;
    async fn write_api(
        &self,
        path: impl AsRef<str> + Send,
        data: Option<Map<String, Value>>,
    ) -> Result<Option<Response>, MegaError>;
    async fn delete_api(&self, path: impl AsRef<str> + Send)
    -> Result<Option<Response>, MegaError>;
    async fn write_secret(
        &self,
        name: &str,
        data: Option<Map<String, Value>>,
    ) -> Result<(), MegaError>;
    async fn read_secret(&self, name: &str) -> Result<Option<Map<String, Value>>, MegaError>;
    async fn delete_secret(&self, name: &str) -> Result<(), MegaError>;
}

impl VaultCore {
    pub async fn new(ctx: Storage) -> Self {
        let dir = common::config::mega_base().join("vault");
        let key_path = dir.join(CORE_KEY_FILE);
        tracing::info!("{key_path:?}");
        std::fs::create_dir_all(&dir).expect("Failed to create vault directory");
        Self::config(ctx.clone(), key_path).await
    }

    pub async fn config(ctx: Storage, key_path: PathBuf) -> Self {
        let backend: Arc<dyn Backend> = Arc::new(JupiterBackend::new(ctx.clone()));
        let seal_config = libvault::core::SealConfig {
            secret_shares: 10,
            secret_threshold: 5,
        };

        let rvault =
            RustyVault::new(backend.clone(), None).expect("Failed to create RustyVault instance");

        let core_key = if !key_path.exists() {
            println!("Vault core key file does not exist, clearing database and reinitializing...");

            let vault_storage = ctx.vault_storage();
            vault_storage
                .delete_all()
                .await
                .expect("Failed to clear vault storage");

            let result = rvault
                .init(&seal_config)
                .await
                .expect("Failed to initialize vault");
            println!(
                "Vault core initialized with root token: {}",
                result.root_token
            );

            let core_key = CoreKey {
                secret_shares: Vec::from(&result.secret_shares[..]),
                root_token: result.root_token.clone(),
            };

            println!(
                "[vault] Creating new core_key.json at: {}",
                key_path.display()
            );

            let file = std::fs::File::create(&key_path).unwrap();
            serde_json::to_writer_pretty(file, &core_key).unwrap();

            core_key
        } else {
            println!("Using existing vault core key file: {}", key_path.display());
            let key_data = std::fs::read(&key_path).expect("Failed to read vault core key file");
            serde_json::from_slice::<CoreKey>(&key_data).expect("Failed to deserialize core key")
        };

        for i in 0..seal_config.secret_threshold {
            let key = &core_key.secret_shares[i as usize];
            let unseal = rvault.unseal(&[key.as_slice()]).await;
            assert!(unseal.is_ok(), "Unseal error: {:?}", unseal.err());
        }

        log::debug!(
            "Vault core initialized with root token: {}",
            core_key.root_token
        );

        let rvault = rvault.into();
        let key = Arc::new(core_key);

        Self { rvault, key }
    }
}

#[async_trait]
impl VaultCoreInterface for VaultCore {
    fn token(&self) -> &str {
        &self.key.root_token
    }

    async fn read_api(&self, path: impl AsRef<str> + Send) -> Result<Option<Response>, MegaError> {
        self.rvault
            .read(self.token().into(), path.as_ref())
            .await
            .map_err(|_| MegaError::Other("Failed to read from vault API".to_string()))
    }

    async fn write_api(
        &self,
        path: impl AsRef<str> + Send,
        data: Option<Map<String, Value>>,
    ) -> Result<Option<Response>, MegaError> {
        self.rvault
            .write(self.token().into(), path.as_ref(), data)
            .await
            .map_err(|e| MegaError::Other(format!("Failed to write to vault API: {e}")))
    }

    async fn delete_api(
        &self,
        path: impl AsRef<str> + Send,
    ) -> Result<Option<Response>, MegaError> {
        self.rvault
            .delete(self.token().into(), path.as_ref(), None)
            .await
            .map_err(|_| MegaError::Other("Failed to delete from vault API".to_string()))
    }

    async fn write_secret(
        &self,
        name: &str,
        data: Option<Map<String, Value>>,
    ) -> Result<(), MegaError> {
        self.write_api(format!("secret/{name}"), data)
            .await
            .map_err(|e| MegaError::Other(format!("Failed to write secret: {name}, {e}")))?;
        Ok(())
    }

    async fn read_secret(&self, name: &str) -> Result<Option<Map<String, Value>>, MegaError> {
        let resp = self
            .read_api(format!("secret/{name}"))
            .await
            .map_err(|_| MegaError::Other(format!("Failed to read secret: {name}")))?;

        Ok(resp.and_then(|r| r.data))
    }

    async fn delete_secret(&self, name: &str) -> Result<(), MegaError> {
        self.delete_api(format!("secret/{name}"))
            .await
            .map_err(|_| MegaError::Other(format!("Failed to delete secret: {name}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use jupiter::tests::test_storage;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_vault_core_initialization() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");
        let key_path = temp_dir.path().join(CORE_KEY_FILE);
        println!("Key path: {key_path:?}");
        let storage = test_storage(temp_dir.path()).await;
        let vault_core = VaultCore::config(storage, key_path).await;

        assert!(
            !vault_core.token().is_empty(),
            "Vault core token should not be empty"
        );
        assert!(
            vault_core.rvault.core.load().inited().await.unwrap(),
            "Vault core should be initialized"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_vault_api() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");
        let key_path = temp_dir.path().join(CORE_KEY_FILE);
        let storage = test_storage(temp_dir.path()).await;
        let vault_core = VaultCore::config(storage, key_path).await;

        let random_pairs = (0..128)
            .map(|_| {
                (
                    rand::random::<u64>().to_string(),
                    rand::random::<u64>().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let data: HashMap<String, Map<String, Value>> = random_pairs
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    serde_json::json!({
                        "data": v,
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                )
            })
            .collect();

        // Write secrets to the vault and store them in a map
        for (name, value) in &data {
            vault_core
                .write_secret(name.as_str(), Some(value.clone()))
                .await
                .expect("Failed to write secret");
        }

        // Read secrets from the vault and verify their values
        for (name, value) in &data {
            let read_value = vault_core
                .read_secret(name.as_str())
                .await
                .expect("Failed to read secret")
                .expect("Secret should exist");
            assert_eq!(
                read_value, *value,
                "Read value does not match written value for {name}"
            );
        }

        // Delete secrets from the vault and verify they are removed
        for name in data.keys() {
            vault_core
                .delete_secret(name.as_str())
                .await
                .expect("Failed to delete secret");

            let read_value = vault_core.read_secret(name.as_str()).await;
            assert!(read_value.is_ok());
            assert!(
                read_value.unwrap().is_none(),
                "Secret {name} should be deleted but still exists"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_vault_reinitialize_after_file_loss() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");
        let key_path = temp_dir.path().join(CORE_KEY_FILE);
        let storage = test_storage(temp_dir.path()).await;

        let vault_core = VaultCore::config(storage.clone(), key_path.clone()).await;

        let secret_data = serde_json::json!({"value": "test"})
            .as_object()
            .unwrap()
            .clone();
        vault_core
            .write_secret("test_key", Some(secret_data))
            .await
            .expect("Failed to write test secret");

        let read_result = vault_core.read_secret("test_key").await;
        assert!(read_result.is_ok());
        assert!(read_result.unwrap().is_some(), "Test secret should exist");

        std::fs::remove_file(&key_path).expect("Failed to delete key file");

        let vault_core2 = VaultCore::config(storage.clone(), key_path.clone()).await;

        assert!(!vault_core2.token().is_empty());
        assert!(vault_core2.rvault.core.load().inited().await.unwrap());

        let read_result2 = vault_core2.read_secret("test_key").await;
        assert!(read_result2.is_ok());
        assert!(
            read_result2.unwrap().is_none(),
            "Old data should be cleared after reinitialize"
        );

        let new_secret_data = serde_json::json!({"value": "new"})
            .as_object()
            .unwrap()
            .clone();
        vault_core2
            .write_secret("new_key", Some(new_secret_data))
            .await
            .expect("Failed to write new secret");
    }
}
