use async_trait::async_trait;
use jupiter::storage::Storage;
use libvault::storage::Backend;

pub struct JupiterBackend {
    ctx: Storage,
}

impl JupiterBackend {
    pub fn new(ctx: Storage) -> Self {
        JupiterBackend { ctx }
    }
}

#[async_trait]
impl Backend for JupiterBackend {
    async fn list(&self, prefix: &str) -> Result<Vec<String>, libvault::errors::RvError> {
        let service = self.ctx.vault_storage();
        let prefix = prefix.to_string();
        service
            .list_keys(prefix)
            .await
            .map_err(|_| libvault::errors::RvError::ErrPhysicalBackendKeyInvalid)
    }

    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<libvault::storage::BackendEntry>, libvault::errors::RvError> {
        let service = self.ctx.vault_storage();
        let key = key.to_string();
        service
            .load(key)
            .await
            .map(|opt| {
                opt.and_then(|model| {
                    libvault::storage::BackendEntry {
                        key: model.key,
                        value: model.value,
                    }
                    .into()
                })
            })
            .map_err(|_| libvault::errors::RvError::ErrPhysicalBackendKeyInvalid)
    }

    async fn put(
        &self,
        entry: &libvault::storage::BackendEntry,
    ) -> Result<(), libvault::errors::RvError> {
        let service = self.ctx.vault_storage();
        let entry_clone = entry.clone();
        service
            .save(entry_clone.key, entry_clone.value)
            .await
            .map(|_| ())
            .map_err(|_| libvault::errors::RvError::ErrPhysicalBackendKeyInvalid)
    }

    async fn delete(&self, key: &str) -> Result<(), libvault::errors::RvError> {
        let service = self.ctx.vault_storage();
        let key = key.to_string();
        service
            .delete(key)
            .await
            .map(|_| ())
            .map_err(|_| libvault::errors::RvError::ErrPhysicalBackendKeyInvalid)
    }
}
