use std::pin::Pin;

use bytes::Bytes;
use common::errors::{GitLFSError, MegaError};
use futures::Stream;

use super::context::LfsApplicationService;
use crate::lfs::{
    handler,
    lfs_structs::{
        BatchRequest, BatchResponse, Lock, LockList, LockListQuery, LockRequest, RequestObject,
        UnlockRequest, VerifiableLockList, VerifiableLockRequest,
    },
};

impl LfsApplicationService {
    #[cfg(feature = "fastcdc")]
    pub fn media_service(&self) -> &jupiter::service::lfs_service::LfsService {
        &self.ctx.storage().lfs_service
    }

    pub async fn lfs_retrieve_lock(&self, query: LockListQuery) -> Result<LockList, GitLFSError> {
        handler::lfs_retrieve_lock(self.ctx.storage().lfs_db_storage(), query).await
    }

    pub async fn lfs_verify_lock(
        &self,
        req: VerifiableLockRequest,
    ) -> Result<VerifiableLockList, MegaError> {
        handler::lfs_verify_lock(self.ctx.storage().lfs_db_storage(), req).await
    }

    pub async fn lfs_create_lock(&self, req: LockRequest) -> Result<Lock, GitLFSError> {
        handler::lfs_create_lock(self.ctx.storage().lfs_db_storage(), req).await
    }

    pub async fn lfs_delete_lock(&self, id: &str, req: UnlockRequest) -> Result<Lock, GitLFSError> {
        handler::lfs_delete_lock(self.ctx.storage().lfs_db_storage(), id, req).await
    }

    pub async fn lfs_process_batch(
        &self,
        request: BatchRequest,
        listen_addr: &str,
    ) -> Result<BatchResponse, GitLFSError> {
        handler::lfs_process_batch(&self.ctx.storage().lfs_service, request, listen_addr).await
    }

    pub async fn lfs_download_object(
        &self,
        oid: String,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, GitLFSError>> + Send>>, GitLFSError> {
        let service = self.ctx.storage().lfs_service.clone();
        let stream = handler::lfs_download_object(service, oid).await?;
        Ok(Box::pin(stream))
    }

    pub async fn lfs_upload_object(
        &self,
        req_obj: &RequestObject,
        body_bytes: Vec<u8>,
    ) -> Result<(), GitLFSError> {
        handler::lfs_upload_object(&self.ctx.storage().lfs_service, req_obj, body_bytes).await
    }
}
