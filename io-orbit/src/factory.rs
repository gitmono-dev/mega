use std::{
    fs::{create_dir_all, exists},
    sync::Arc,
};

use common::{
    config::{ObjectStorageBackend, ObjectStorageConfig, S3Config},
    errors::MegaError,
};
use object_store::{
    aws::{AmazonS3, AmazonS3Builder},
    gcp::GoogleCloudStorageBuilder,
    local::LocalFileSystem,
};

use crate::{
    adapter::{BackendStore, ObjectStoreAdapter, UploadStrategy},
    log_storage::LogStorage,
    object_storage::MegaObjectStorage,
};

pub trait MegaObjectStorageWithLog: MegaObjectStorage + LogStorage {}

impl<T: MegaObjectStorage + LogStorage> MegaObjectStorageWithLog for T {}

#[derive(Clone)]
pub struct MegaObjectStorageWrapper {
    pub inner: Arc<dyn MegaObjectStorageWithLog>,
}

impl MegaObjectStorageWrapper {
    pub fn new(inner: Arc<dyn MegaObjectStorageWithLog>) -> Self {
        Self { inner }
    }

    pub fn supports_presigned_urls(&self) -> bool {
        MegaObjectStorage::supports_presigned_urls(&*self.inner)
    }

    pub fn mock() -> Self {
        if !exists("/tmp/mega_test_object_storage").expect("mock err") {
            create_dir_all("/tmp/mega_test_object_storage").expect("init mock file err")
        }
        let fs = LocalFileSystem::new_with_prefix("/tmp/mega_test_object_storage")
            .expect("mock init error");
        let store = BackendStore::Local(Arc::new(fs));
        let adapter = Arc::new(ObjectStoreAdapter {
            store,
            presign_store: None,
            upload_strategy: UploadStrategy::SinglePut,
        });
        MegaObjectStorageWrapper::new(adapter)
    }
}

pub struct ObjectStorageFactory;

impl ObjectStorageFactory {
    /// Builds object storage from [`ObjectStorageConfig::storage_type`] and nested credentials/paths.
    pub async fn build(cfg: &ObjectStorageConfig) -> Result<MegaObjectStorageWrapper, MegaError> {
        match cfg.storage_type {
            ObjectStorageBackend::S3 => build_s3_like(cfg, false).await,
            ObjectStorageBackend::S3Compatible => build_s3_like(cfg, true).await,
            ObjectStorageBackend::Gcs => build_gcs(cfg).await,
            ObjectStorageBackend::Local => build_local(cfg).await,
        }
    }
}

fn build_amazon_s3(
    s3_cfg: &S3Config,
    endpoint: &str,
    compatible: bool,
) -> Result<AmazonS3, MegaError> {
    let mut builder = AmazonS3Builder::new()
        .with_region(&s3_cfg.region)
        .with_bucket_name(&s3_cfg.bucket)
        .with_access_key_id(&s3_cfg.access_key_id)
        .with_secret_access_key(&s3_cfg.secret_access_key);

    if compatible || !endpoint.is_empty() {
        if !endpoint.is_empty() {
            builder = builder.with_endpoint(endpoint);
        }
        if compatible {
            // S3-compatible (RustFS/MinIO): path-style; allow plain HTTP for
            // in-cluster endpoints. HTTPS public endpoints still work with
            // allow_http(true).
            builder = builder
                .with_allow_http(true)
                .with_virtual_hosted_style_request(false);
        } else if endpoint.starts_with("http://") {
            builder = builder.with_allow_http(true);
        }
    }

    builder.build().map_err(|e| MegaError::Other(e.to_string()))
}

/// Shared S3 / S3-compatible construction (differs only by endpoint and upload strategy).
async fn build_s3_like(
    cfg: &ObjectStorageConfig,
    compatible: bool,
) -> Result<MegaObjectStorageWrapper, MegaError> {
    let s3_cfg = &cfg.s3;
    let endpoint = if compatible {
        s3_cfg.endpoint_url.as_str()
    } else {
        // Real AWS: empty endpoint uses the default regional endpoint.
        ""
    };
    let s3 = build_amazon_s3(s3_cfg, endpoint, compatible)?;

    let presign_store = {
        let presign_ep = s3_cfg.presign_endpoint_url.trim();
        if presign_ep.is_empty() {
            None
        } else {
            Some(Arc::new(build_amazon_s3(s3_cfg, presign_ep, compatible)?))
        }
    };

    let store = BackendStore::S3(Arc::new(s3));
    let upload_strategy = if compatible {
        UploadStrategy::SinglePut
    } else {
        UploadStrategy::Multipart
    };
    let adapter = Arc::new(ObjectStoreAdapter {
        store,
        presign_store,
        upload_strategy,
    });

    Ok(MegaObjectStorageWrapper::new(adapter))
}

async fn build_gcs(cfg: &ObjectStorageConfig) -> Result<MegaObjectStorageWrapper, MegaError> {
    let gcp_cfg = cfg.gcs.clone();
    let gcs = GoogleCloudStorageBuilder::from_env()
        .with_bucket_name(&gcp_cfg.bucket)
        .build()
        .map_err(|e| MegaError::Other(e.to_string()))?;
    let store = BackendStore::Gcs(Arc::new(gcs));
    let adapter = Arc::new(ObjectStoreAdapter {
        store,
        presign_store: None,
        upload_strategy: UploadStrategy::SinglePut,
    });

    Ok(MegaObjectStorageWrapper::new(adapter))
}

async fn build_local(cfg: &ObjectStorageConfig) -> Result<MegaObjectStorageWrapper, MegaError> {
    if !exists(&cfg.local.root_dir)? {
        create_dir_all(&cfg.local.root_dir)?
    }
    let fs = LocalFileSystem::new_with_prefix(&cfg.local.root_dir)
        .map_err(|e| MegaError::Other(e.to_string()))?;

    let store = BackendStore::Local(Arc::new(fs));
    let adapter = Arc::new(ObjectStoreAdapter {
        store,
        presign_store: None,
        upload_strategy: UploadStrategy::SinglePut,
    });

    Ok(MegaObjectStorageWrapper::new(adapter))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use common::config::{
        GcsConfig, LocalConfig, ObjectStorageBackend, ObjectStorageConfig, S3Config,
    };
    use object_store::{path::Path, signer::Signer};
    use reqwest::Method;

    use super::build_amazon_s3;
    use crate::object_storage::{ObjectKey, ObjectNamespace};

    fn sample_s3_config(endpoint: &str, presign: &str) -> S3Config {
        S3Config {
            region: "us-east-1".into(),
            bucket: "buck2hub-assets".into(),
            access_key_id: "testkey".into(),
            secret_access_key: "testsecret".into(),
            endpoint_url: endpoint.into(),
            presign_endpoint_url: presign.into(),
        }
    }

    #[tokio::test]
    async fn signed_url_uses_endpoint_url_when_presign_unset() {
        let s3_cfg = sample_s3_config("http://rustfs.internal:9000", "");
        let s3 = build_amazon_s3(&s3_cfg, &s3_cfg.endpoint_url, true).unwrap();
        let path = Path::from("orion-images/abc/debian-13-buck2.qcow2");
        let url = s3
            .signed_url(Method::GET, &path, Duration::from_secs(60))
            .await
            .unwrap()
            .to_string();
        assert!(
            url.starts_with("http://rustfs.internal:9000/"),
            "expected internal host, got {url}"
        );
    }

    #[tokio::test]
    async fn signed_url_uses_presign_endpoint_when_set() {
        let s3_cfg = sample_s3_config(
            "http://rustfs.mega-dev.svc.cluster.local:9000",
            "https://rustfs.xuanwu.openatom.cn",
        );
        let presign = build_amazon_s3(&s3_cfg, &s3_cfg.presign_endpoint_url, true).unwrap();
        let path = Path::from("orion-images/abc/debian-13-buck2.qcow2");
        let url = presign
            .signed_url(Method::GET, &path, Duration::from_secs(60))
            .await
            .unwrap()
            .to_string();
        assert!(
            url.starts_with("https://rustfs.xuanwu.openatom.cn/"),
            "expected public host, got {url}"
        );
        assert!(
            !url.contains("svc.cluster.local"),
            "presign URL must not use in-cluster host: {url}"
        );
    }

    #[tokio::test]
    async fn factory_wires_presign_store_for_compatible() {
        use crate::factory::ObjectStorageFactory;

        let cfg = ObjectStorageConfig {
            storage_type: ObjectStorageBackend::S3Compatible,
            s3: sample_s3_config(
                "http://rustfs.internal:9000",
                "https://rustfs.example.public",
            ),
            gcs: GcsConfig::default(),
            local: LocalConfig::default(),
        };
        let wrapper = ObjectStorageFactory::build(&cfg).await.unwrap();
        let key = ObjectKey {
            namespace: ObjectNamespace::OrionImage,
            key: "deadbeef/debian-13-buck2.qcow2".into(),
        };
        let url = wrapper
            .inner
            .signed_url(&key, Method::GET, Duration::from_secs(120))
            .await
            .unwrap()
            .expect("s3compatible must support presign");
        assert!(
            url.starts_with("https://rustfs.example.public/"),
            "factory should sign with presign_endpoint_url, got {url}"
        );
    }
}
