use std::time::Duration;

use callisto::orion_vm_image;
use common::errors::MegaError;
use io_orbit::{
    factory::MegaObjectStorageWrapper,
    object_storage::{ObjectByteStream, ObjectKey, ObjectMeta, ObjectNamespace},
};
use reqwest::Method;

use crate::storage::{
    base_storage::{BaseStorage, StorageConnector},
    orion_vm_image_storage::{OrionVmImageStorage, UpsertOrionVmImage},
};

/// Presigned GET TTL for scheduler first-pull of multi-GB qcow2 images.
pub const ORION_IMAGE_PRESIGN_TTL_SECS: u64 = 6 * 60 * 60;

#[derive(Clone)]
pub struct OrionVmImageService {
    st: OrionVmImageStorage,
    obj_storage: MegaObjectStorageWrapper,
}

impl OrionVmImageService {
    pub fn new(base: BaseStorage, obj_storage: MegaObjectStorageWrapper) -> Self {
        Self {
            st: OrionVmImageStorage { base },
            obj_storage,
        }
    }

    pub fn mock() -> Self {
        Self::new(BaseStorage::mock(), MegaObjectStorageWrapper::mock())
    }

    pub fn supports_presigned_urls(&self) -> bool {
        self.obj_storage.supports_presigned_urls()
    }

    pub async fn list(&self) -> Result<Vec<orion_vm_image::Model>, MegaError> {
        self.st.list_all().await
    }

    /// Newest catalog row by `created_at` (same order as [`Self::list`]).
    pub async fn latest(&self) -> Result<Option<orion_vm_image::Model>, MegaError> {
        Ok(self.list().await?.into_iter().next())
    }

    pub async fn get(&self, id: &str) -> Result<Option<orion_vm_image::Model>, MegaError> {
        self.st.find_by_id(id).await
    }

    pub async fn upsert(
        &self,
        input: UpsertOrionVmImage,
    ) -> Result<orion_vm_image::Model, MegaError> {
        self.st.upsert(input).await
    }

    /// Delete RustFS/local objects first, then the catalog row.
    /// Object-not-found is treated as success so a prior partial delete can finish.
    pub async fn delete(&self, id: &str) -> Result<Option<orion_vm_image::Model>, MegaError> {
        let Some(model) = self.st.find_by_id(id).await? else {
            return Ok(None);
        };

        let qcow2 = ObjectKey {
            namespace: ObjectNamespace::OrionImage,
            key: model.object_key.clone(),
        };
        match self.obj_storage.inner.delete(&qcow2).await {
            Ok(()) => {}
            Err(MegaError::ObjStorageNotFound(_)) => {
                tracing::info!(
                    "orion image object already absent: {}",
                    qcow2.default_sharding()
                );
            }
            Err(e) => {
                return Err(MegaError::ObjStorage(format!(
                    "failed to delete orion image object {}: {e}",
                    qcow2.default_sharding()
                )));
            }
        }

        if let Some(info_key) = &model.info_object_key {
            let info = ObjectKey {
                namespace: ObjectNamespace::OrionImage,
                key: info_key.clone(),
            };
            match self.obj_storage.inner.delete(&info).await {
                Ok(()) => {}
                Err(MegaError::ObjStorageNotFound(_)) => {
                    tracing::info!(
                        "orion image info object already absent: {}",
                        info.default_sharding()
                    );
                }
                Err(e) => {
                    return Err(MegaError::ObjStorage(format!(
                        "failed to delete orion image info object {}: {e}",
                        info.default_sharding()
                    )));
                }
            }
        }

        self.st.delete_by_id(id).await
    }

    pub async fn signed_get_url(&self, model: &orion_vm_image::Model) -> Result<String, MegaError> {
        self.signed_url_for_key(&model.object_key, Method::GET)
            .await
    }

    pub async fn signed_put_url(&self, object_key: &str) -> Result<String, MegaError> {
        self.signed_url_for_key(object_key, Method::PUT).await
    }

    async fn signed_url_for_key(
        &self,
        object_key: &str,
        method: Method,
    ) -> Result<String, MegaError> {
        let key = ObjectKey {
            namespace: ObjectNamespace::OrionImage,
            key: object_key.to_string(),
        };
        let url = self
            .obj_storage
            .inner
            .signed_url(
                &key,
                method,
                Duration::from_secs(ORION_IMAGE_PRESIGN_TTL_SECS),
            )
            .await?;
        url.ok_or_else(|| {
            MegaError::ObjStorage(
                "object storage does not support presigned URLs; configure S3-compatible RustFS"
                    .into(),
            )
        })
    }

    /// Stream an object into the Orion image namespace (bounded buffering / multipart).
    pub async fn put_object_stream(
        &self,
        object_key: &str,
        data: ObjectByteStream,
    ) -> Result<(), MegaError> {
        let key = ObjectKey {
            namespace: ObjectNamespace::OrionImage,
            key: object_key.trim().trim_start_matches('/').to_string(),
        };
        if key.key.is_empty() || key.key.contains("..") {
            return Err(MegaError::Other("invalid object_key".into()));
        }
        self.obj_storage
            .inner
            .put_stream_bounded(&key, data, ObjectMeta::default())
            .await
    }

    /// Build catalog object keys: `{digest_hex}/{image_name}.qcow2` (+ optional info sidecar).
    pub fn object_keys(digest: &str, image_name: &str) -> (String, String) {
        let hex = digest_hex(digest);
        let name = image_name.trim().trim_end_matches(".qcow2");
        let name = if name.is_empty() {
            "debian-13-buck2"
        } else {
            name
        };
        (
            format!("{hex}/{name}.qcow2"),
            format!("{hex}/image-info.json"),
        )
    }
}

/// Strip optional `sha256:` / `sha512:` prefix for object-key layout.
pub fn digest_hex(digest: &str) -> &str {
    digest
        .strip_prefix("sha256:")
        .or_else(|| digest.strip_prefix("sha512:"))
        .unwrap_or(digest)
}
