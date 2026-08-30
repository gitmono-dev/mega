use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use callisto::lfs_locks;
use chrono::prelude::*;
use common::errors::{GitLFSError, MegaError};
use futures::{Stream, StreamExt};
use io_orbit::{
    factory::MegaObjectStorageWrapper,
    object_storage::{ObjectKey, ObjectMeta, ObjectNamespace},
};
use jupiter::{
    service::lfs_service::LfsService, storage::lfs_db_storage::LfsDbStorage,
    utils::into_obj_stream::IntoObjectStream,
};
use rand::prelude::*;
use reqwest::Method;

use crate::lfs::lfs_structs::{
    BatchRequest, BatchResponse, Lock, LockList, LockListQuery, LockRequest, MetaObject,
    ObjectError, Operation, RequestObject, ResCondition, ResponseObject, TransferMode,
    UnlockRequest, VerifiableLockList, VerifiableLockRequest,
};

pub async fn lfs_retrieve_lock(
    storage: LfsDbStorage,
    query: LockListQuery,
) -> Result<LockList, GitLFSError> {
    let mut lock_list = LockList {
        locks: vec![],
        next_cursor: "".to_string(),
    };
    match lfs_get_filtered_locks(
        storage,
        &query.refspec,
        &query.path,
        &query.cursor,
        &query.limit,
    )
    .await
    {
        Ok((locks, next)) => {
            lock_list.locks = locks;
            lock_list.next_cursor = next;
            Ok(lock_list)
        }
        // Client-input errors (e.g. a malformed `limit`) must reach the router
        // unmasked so `map_lfs_error` can classify them as 400; only genuine
        // lookup failures are hidden behind the generic message.
        Err(GitLFSError::GeneralError(msg)) if msg.starts_with("Invalid") => {
            Err(GitLFSError::GeneralError(msg))
        }
        Err(_) => Err(GitLFSError::GeneralError(
            "Lookup operation failed!".to_string(),
        )),
    }
}

pub async fn lfs_verify_lock(
    storage: LfsDbStorage,
    req: VerifiableLockRequest,
) -> Result<VerifiableLockList, MegaError> {
    let mut limit = req.limit.unwrap_or(0);
    if limit == 0 {
        limit = 100;
    }
    let res = lfs_get_filtered_locks(
        storage,
        &req.refs.name,
        "",
        &req.cursor.clone().unwrap_or("".to_string()).to_string(),
        &limit.to_string(),
    )
    .await;

    let mut lock_list = VerifiableLockList {
        ours: vec![],
        theirs: vec![],
        next_cursor: "".to_string(),
    };
    match res {
        Ok((locks, next_cursor)) => {
            lock_list.next_cursor = next_cursor;

            for lock in locks.iter() {
                if Option::is_none(&lock.owner) {
                    lock_list.ours.push(lock.clone());
                } else {
                    lock_list.theirs.push(lock.clone());
                }
            }
        }
        Err(_) => return Err(MegaError::Other("Lookup operation failed!".to_string())),
    };
    Ok(lock_list)
}

pub async fn lfs_create_lock(storage: LfsDbStorage, req: LockRequest) -> Result<Lock, GitLFSError> {
    let res = lfs_get_filtered_locks(
        storage.clone(),
        &req.refs.name,
        &req.path.to_string(),
        "",
        "1",
    )
    .await;

    match res {
        Ok((locks, _)) => {
            if !locks.is_empty() {
                return Err(GitLFSError::GeneralError("Lock already exist".to_string()));
            }
        }
        Err(_) => {
            return Err(GitLFSError::GeneralError(
                "Failed when filtering locks!".to_string(),
            ));
        }
    };

    let lock = Lock {
        id: {
            let mut random_num = String::new();
            let mut rng = rand::rng();
            for _ in 0..8 {
                random_num += &(rng.random_range(0..9)).to_string();
            }
            random_num
        },
        path: req.path.to_owned(),
        owner: None,
        locked_at: {
            let locked_at: DateTime<Utc> = Utc::now();
            locked_at.to_rfc3339()
        },
    };

    match lfs_add_lock(storage.clone(), &req.refs.name, vec![lock.clone()]).await {
        Ok(_) => Ok(lock),
        Err(_) => Err(GitLFSError::GeneralError(
            "Failed when adding locks!".to_string(),
        )),
    }
}

pub async fn lfs_delete_lock(
    storage: LfsDbStorage,
    id: &str,
    unlock_request: UnlockRequest,
) -> Result<Lock, GitLFSError> {
    if id.is_empty() {
        return Err(GitLFSError::GeneralError("Invalid lock id!".to_string()));
    }
    let res = delete_lock(
        storage,
        &unlock_request.refs.name,
        None,
        id,
        unlock_request.force.unwrap_or(false),
    )
    .await;
    match res {
        Ok(deleted_lock) => {
            if deleted_lock.id.is_empty()
                && deleted_lock.path.is_empty()
                && deleted_lock.owner.is_none()
                && deleted_lock.locked_at == DateTime::<Utc>::MIN_UTC.to_rfc3339()
            {
                Err(GitLFSError::GeneralError(
                    "Unable to find lock!".to_string(),
                ))
            } else {
                Ok(deleted_lock)
            }
        }
        Err(_) => Err(GitLFSError::GeneralError(
            "Delete operation failed!".to_string(),
        )),
    }
}

/// The basic protocol accepts only SHA-256 object IDs, never storage paths.
/// Keep this check independent of optional transports so private storage keys
/// cannot be addressed through the standard LFS endpoints.
pub fn validate_object_oid(oid: &str) -> Result<(), GitLFSError> {
    if oid.len() != 64
        || !oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(GitLFSError::GeneralError(
            "Invalid LFS object ID: expected 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn validate_object_size(size: i64) -> Result<(), GitLFSError> {
    if size < 0 {
        return Err(GitLFSError::GeneralError(
            "Invalid LFS object size: expected a non-negative size".into(),
        ));
    }
    Ok(())
}

fn validate_request_object(object: &RequestObject) -> Result<(), GitLFSError> {
    validate_object_oid(&object.oid)?;
    validate_object_size(object.size)
}

fn lfs_database_error(error: MegaError) -> GitLFSError {
    tracing::error!("LFS metadata storage operation failed: {error}");
    GitLFSError::GeneralError("LFS metadata storage operation failed".into())
}

/// Reference:
///     1. [Git LFS Batch API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md)
pub async fn lfs_process_batch(
    service: &LfsService,
    request: BatchRequest,
    listen_addr: &str,
) -> Result<BatchResponse, GitLFSError> {
    let objects = request.objects;

    let mut response_objects = Vec::new();
    let file_storage = service.obj_storage.clone();
    let db_storage = service.lfs_storage.clone();
    for object in objects {
        if let Err(error) = validate_request_object(&object) {
            response_objects.push(ResponseObject::failed_with_err(
                &object,
                ObjectError {
                    code: 400,
                    message: error.to_string(),
                },
            ));
            continue;
        }
        let meta_res = lfs_get_meta(&db_storage, &object.oid).await?;
        let meta = match meta_res {
            Some(meta) => meta,
            None => {
                if request.operation == Operation::Upload {
                    // Save to database if not exist.
                    let meta = MetaObject::new(&object);
                    db_storage
                        .new_lfs_object(meta.clone().into())
                        .await
                        .map_err(lfs_database_error)?;
                    meta
                } else {
                    response_objects.push(ResponseObject::failed_with_err(
                        &object,
                        ObjectError {
                            code: 404,
                            message: "Not found".to_owned(),
                        },
                    ));
                    continue;
                }
            }
        };
        let file_exist = lfs_object_exists(&file_storage, &meta.oid).await;
        let download_url = match lfs_download_url(&file_storage, &meta.oid, listen_addr).await {
            Ok(url) => url,
            Err(e) => {
                tracing::error!("Failed to generate download URL for {}: {}", meta.oid, e);
                response_objects.push(ResponseObject::failed_with_err(
                    &object,
                    ObjectError {
                        code: 500,
                        message: format!("Failed to generate download URL: {}", e),
                    },
                ));
                continue;
            }
        };
        let upload_url = match lfs_upload_url(&file_storage, &meta.oid, listen_addr).await {
            Ok(url) => url,
            Err(e) => {
                tracing::error!("Failed to generate upload URL for {}: {}", meta.oid, e);
                response_objects.push(ResponseObject::failed_with_err(
                    &object,
                    ObjectError {
                        code: 500,
                        message: format!("Failed to generate upload URL: {}", e),
                    },
                ));
                continue;
            }
        };

        response_objects.push(ResponseObject::new(
            &meta,
            ResCondition {
                file_exist,
                operation: request.operation.clone(),
                use_tus: false,
            },
            &download_url,
            &upload_url,
        ));
    }

    Ok(BatchResponse {
        transfer: TransferMode::BASIC,
        objects: response_objects,
        hash_algo: "sha256".to_string(),
    })
}

/// Upload object to storage.
/// if server enable split, split the object and upload each part to storage, save the relationship to database.
pub async fn lfs_upload_object(
    service: &LfsService,
    req_obj: &RequestObject,
    body_bytes: Vec<u8>,
) -> Result<(), GitLFSError> {
    validate_request_object(req_obj)?;
    let db_storage: LfsDbStorage = service.lfs_storage.clone();

    let meta = if let Some(meta) = lfs_get_meta(&db_storage, &req_obj.oid).await? {
        tracing::debug!("upload lfs object {} size: {}", meta.oid, meta.size);
        meta
    } else {
        return Err(GitLFSError::GeneralError(String::from("Not found ")));
    };

    let key = lfs_object_key(&meta.oid);
    let size = meta.size;
    let res = service
        .obj_storage
        .inner
        .put_stream(
            &key,
            body_bytes.into_stream(),
            ObjectMeta {
                size,
                ..Default::default()
            },
        )
        .await;
    if let Err(_e) = res {
        if let Err(delete_err) = lfs_delete_meta(&db_storage, req_obj).await {
            tracing::error!(
                "Failed to cleanup LFS metadata for oid {} after upload failure: {}",
                meta.oid,
                delete_err
            );
        }
        return Err(GitLFSError::GeneralError(String::from(
            "Header not acceptable!",
        )));
    }
    Ok(())
}

/// Download object from storage.
/// when server enable split,  if OID is a complete object, then splice the object and return it.
pub async fn lfs_download_object(
    service: LfsService,
    oid: String,
) -> Result<impl Stream<Item = Result<Bytes, GitLFSError>>, GitLFSError> {
    let db_storage = service.lfs_storage.clone();
    let file_storage = service.obj_storage.clone();

    let meta = lfs_get_meta(&db_storage, &oid).await?;
    match meta {
        Some(meta) => {
            // Fetch object from unified object storage.
            let key = lfs_object_key(&meta.oid);
            let (stream, _meta) = match file_storage.inner.get_stream(&key).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("Failed to get LFS object {}: {}", meta.oid, e);
                    return Err(GitLFSError::GeneralError(format!(
                        "Failed to retrieve object: {}",
                        e
                    )));
                }
            };
            // Map storage's `ObjectByteStream` into the expected `GitLFSError` stream type.
            let mapped = stream.map(|chunk| match chunk {
                Ok(bytes) => Ok(bytes),
                Err(e) => Err(GitLFSError::GeneralError(format!(
                    "Stream error while reading object: {}",
                    e
                ))),
            });
            Ok(mapped)
        }
        None => Err(GitLFSError::GeneralError(format!(
            "LFS object not found: {}",
            oid
        ))),
    }
}

async fn lfs_get_filtered_locks(
    storage: LfsDbStorage,
    refspec: &str,
    path: &str,
    cursor: &str,
    limit: &str,
) -> Result<(Vec<Lock>, String), GitLFSError> {
    let mut locks = (lfs_get_locks(storage, refspec).await).unwrap_or_default();

    tracing::debug!("Locks retrieved: {:?}", locks);

    if !cursor.is_empty() {
        let mut last_seen = -1;
        for (i, v) in locks.iter().enumerate() {
            if v.id == *cursor {
                last_seen = i as i32;
                break;
            }
        }

        if last_seen > -1 {
            locks = locks.split_off(last_seen as usize);
        } else {
            // Cursor not found.
            return Err(GitLFSError::GeneralError("".to_string()));
        }
    }

    if !path.is_empty() {
        let mut filterd = Vec::<Lock>::new();
        for lock in locks.iter() {
            if lock.path == *path {
                filterd.push(Lock {
                    id: lock.id.to_owned(),
                    path: lock.path.to_owned(),
                    owner: lock.owner.clone(),
                    locked_at: lock.locked_at.to_owned(),
                });
            }
        }
        locks = filterd;
    }

    apply_lock_limit(locks, limit)
}

/// Applies the `limit` parameter to an ordered lock list, returning the page and
/// the id of the first lock past the page (empty when no further page exists).
///
/// The limit comes straight from the query string, so anything non-numeric
/// (including negative values) is rejected instead of being parsed leniently.
fn apply_lock_limit(locks: Vec<Lock>, limit: &str) -> Result<(Vec<Lock>, String), GitLFSError> {
    let mut next = "".to_string();
    let mut locks = locks;
    if !limit.is_empty() {
        let size = limit
            .parse::<usize>()
            .map_err(|_| GitLFSError::GeneralError(format!("Invalid limit parameter: {limit}")))?;
        let size = size.min(locks.len());

        if size + 1 < locks.len() {
            locks[size].id.clone_into(&mut next);
        }
        let _ = locks.split_off(size);
    }

    Ok((locks, next))
}

async fn lfs_get_locks(storage: LfsDbStorage, refspec: &str) -> Result<Vec<Lock>, GitLFSError> {
    let result = storage.get_lock_by_id(refspec).await.unwrap();
    match result {
        Some(val) => {
            let data = val.data;
            let locks: Vec<Lock> = serde_json::from_str(&data).unwrap();
            Ok(locks)
        }
        None => Err(GitLFSError::GeneralError("".to_string())),
    }
}

async fn lfs_add_lock(
    storage: LfsDbStorage,
    repo: &str,
    locks: Vec<Lock>,
) -> Result<(), GitLFSError> {
    let result = storage.get_lock_by_id(repo).await.unwrap();

    match result {
        // Update
        Some(val) => {
            let d = val.data.to_owned();
            let mut locks_from_data = if !d.is_empty() {
                let locks_from_data: Vec<Lock> = serde_json::from_str(&d).unwrap();
                locks_from_data
            } else {
                vec![]
            };
            let mut locks = locks;
            locks_from_data.append(&mut locks);

            locks_from_data.sort_by(|a, b| {
                a.locked_at
                    .partial_cmp(&b.locked_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let d = serde_json::to_string(&locks_from_data).unwrap();

            // must turn into `ActiveModel` before modify, or update failed.
            // let mut val = val.into_active_model();
            // val.data = Set(d);
            let res = storage.update_lock(val, &d).await;
            match res.is_ok() {
                true => Ok(()),
                false => Err(GitLFSError::GeneralError("".to_string())),
            }
        }
        // Insert
        None => {
            let mut locks = locks;
            locks.sort_by(|a, b| {
                a.locked_at
                    .partial_cmp(&b.locked_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let data = serde_json::to_string(&locks).unwrap();
            let lock_to = lfs_locks::Model {
                id: repo.to_owned(),
                data: data.to_owned(),
            };

            let res = storage.new_lock(lock_to).await;
            match res.is_ok() {
                true => Ok(()),
                false => Err(GitLFSError::GeneralError("".to_string())),
            }
        }
    }
}

async fn lfs_get_meta(
    storage: &LfsDbStorage,
    oid: &str,
) -> Result<Option<MetaObject>, GitLFSError> {
    validate_object_oid(oid)?;
    let meta: Option<MetaObject> = storage
        .get_lfs_object(oid)
        .await
        .map_err(lfs_database_error)?
        .map(Into::into);
    if let Some(meta) = &meta {
        validate_object_size(meta.size)?;
    }
    Ok(meta)
}

async fn lfs_delete_meta(
    storage: &LfsDbStorage,
    req_obj: &RequestObject,
) -> Result<(), GitLFSError> {
    let res = storage.delete_lfs_object(req_obj.oid.to_owned()).await;
    match res {
        Ok(_) => Ok(()),
        Err(_) => Err(GitLFSError::GeneralError("".to_string())),
    }
}

fn lfs_object_key(oid: &str) -> ObjectKey {
    ObjectKey {
        namespace: ObjectNamespace::Lfs,
        key: oid.to_string(),
    }
}

async fn lfs_object_exists(storage: &MegaObjectStorageWrapper, oid: &str) -> bool {
    let key = lfs_object_key(oid);

    match storage.inner.exists(&key).await {
        Ok(exists) => exists,
        Err(err) => {
            tracing::warn!("Failed to check LFS object {} existence: {}", oid, err);
            false
        }
    }
}

async fn lfs_download_url(
    storage: &MegaObjectStorageWrapper,
    oid: &str,
    hostname: &str,
) -> Result<String, MegaError> {
    let key = lfs_object_key(oid);

    if let Some(url) = storage
        .inner
        .signed_url(&key, Method::GET, Duration::from_secs(3600))
        .await?
    {
        return Ok(url);
    }

    Ok(format!("{}/info/lfs/objects/{}", hostname, oid))
}

async fn lfs_upload_url(
    storage: &MegaObjectStorageWrapper,
    oid: &str,
    hostname: &str,
) -> Result<String, MegaError> {
    let key = lfs_object_key(oid);

    if let Some(url) = storage
        .inner
        .signed_url(&key, Method::PUT, Duration::from_secs(3600))
        .await?
    {
        return Ok(url);
    }

    Ok(format!("{}/info/lfs/objects/{}", hostname, oid))
}

async fn delete_lock(
    storage: LfsDbStorage,
    repo: &str,
    _user: Option<String>,
    id: &str,
    force: bool,
) -> Result<Lock, GitLFSError> {
    let result = storage.get_lock_by_id(repo).await.unwrap();
    match result {
        // Exist, then delete.
        Some(val) => {
            let d = val.data.to_owned();
            let locks_from_data = if !d.is_empty() {
                let locks_from_data: Vec<Lock> = serde_json::from_str(&d).unwrap();
                locks_from_data
            } else {
                vec![]
            };

            let mut new_locks = Vec::<Lock>::new();
            let mut lock_to_delete = Lock {
                id: "".to_owned(),
                path: "".to_owned(),
                owner: None,
                locked_at: {
                    let locked_at: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;
                    locked_at.to_rfc3339()
                },
            };

            for lock in locks_from_data.iter() {
                if lock.id == *id {
                    if Option::is_some(&lock.owner) && !force {
                        return Err(GitLFSError::GeneralError("".to_string()));
                    }
                    lock.id.clone_into(&mut lock_to_delete.id);
                    lock.path.clone_into(&mut lock_to_delete.path);
                    lock_to_delete.owner.clone_from(&lock.owner);
                    lock.locked_at.clone_into(&mut lock_to_delete.locked_at);
                } else if !lock.id.is_empty() {
                    new_locks.push(Lock {
                        id: lock.id.to_owned(),
                        path: lock.path.to_owned(),
                        owner: lock.owner.clone(),
                        locked_at: lock.locked_at.to_owned(),
                    });
                }
            }
            if lock_to_delete.id.is_empty() {
                return Err(GitLFSError::GeneralError("".to_string()));
            }

            // No locks remains, delete the repo from database.
            if new_locks.is_empty() {
                storage.delete_lock_by_id(repo.to_owned()).await;
                return Ok(lock_to_delete);
            }

            // Update remaining locks.
            let data = serde_json::to_string(&new_locks).unwrap();
            let res = storage.update_lock(val, &data).await;
            match res.is_ok() {
                true => Ok(lock_to_delete),
                false => Err(GitLFSError::GeneralError("".to_string())),
            }
        }
        // Not exist, error.
        None => Err(GitLFSError::GeneralError("".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use common::config::{LocalConfig, ObjectStorageBackend, ObjectStorageConfig};
    use io_orbit::factory::ObjectStorageFactory;
    use jupiter::storage::base_storage::{BaseStorage, StorageConnector};
    use sea_orm::{ConnectionTrait, Database};

    use super::*;
    use crate::lfs::lfs_structs::{Action, Ref, ResCondition, ResponseObject};

    async fn lfs_fixture() -> (tempfile::TempDir, LfsService) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared(
            "CREATE TABLE lfs_objects (oid TEXT PRIMARY KEY, size BIGINT NOT NULL, exist BOOLEAN NOT NULL)",
        )
        .await
        .unwrap();
        let config = ObjectStorageConfig {
            storage_type: ObjectStorageBackend::Local,
            local: LocalConfig {
                root_dir: dir.path().to_string_lossy().into_owned(),
            },
            ..Default::default()
        };
        let service = LfsService {
            lfs_storage: LfsDbStorage {
                base: BaseStorage::new(Arc::new(db)),
            },
            obj_storage: ObjectStorageFactory::build(&config).await.unwrap(),
        };
        (dir, service)
    }

    fn batch_request(operation: Operation, objects: Vec<RequestObject>) -> BatchRequest {
        BatchRequest {
            operation,
            transfers: vec!["basic".into()],
            objects,
            hash_algo: "sha256".into(),
        }
    }

    #[tokio::test]
    async fn basic_rejects_invalid_identifiers_and_sizes_before_database_access() {
        let (_dir, service) = lfs_fixture().await;
        let legacy_oid = "b".repeat(64);
        service
            .lfs_storage
            .new_lfs_object(callisto::lfs_objects::Model {
                oid: legacy_oid.clone(),
                size: -1,
                exist: true,
            })
            .await
            .unwrap();
        assert!(matches!(
            lfs_download_object(service.clone(), legacy_oid.clone()).await,
            Err(GitLFSError::GeneralError(message)) if message.starts_with("Invalid")
        ));
        assert!(matches!(
            lfs_upload_object(
                &service,
                &RequestObject { oid: legacy_oid, ..Default::default() },
                vec![],
            ).await,
            Err(GitLFSError::GeneralError(message)) if message.starts_with("Invalid")
        ));
        // Validating these requests must not depend on a working metadata DB.
        service
            .lfs_storage
            .get_connection()
            .execute_unprepared("DROP TABLE lfs_objects")
            .await
            .unwrap();
        let mut requests: Vec<RequestObject> = [
            String::new(),
            "../secret".into(),
            "media-v1/known-scope/chunks/known-hash".into(),
            "media-v1%2Fknown-scope%2Fpending%2Fmanifest".into(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ]
        .into_iter()
        .map(|oid| RequestObject {
            oid,
            size: 1,
            ..Default::default()
        })
        .collect();
        requests.push(RequestObject {
            oid: "a".repeat(64),
            size: -1,
            ..Default::default()
        });

        for operation in [Operation::Upload, Operation::Download] {
            let objects = requests
                .iter()
                .map(|request| RequestObject {
                    oid: request.oid.clone(),
                    size: request.size,
                    ..Default::default()
                })
                .collect();
            let response = lfs_process_batch(
                &service,
                batch_request(operation, objects),
                "http://localhost",
            )
            .await
            .unwrap();
            for object in response.objects {
                assert_eq!(object.error.unwrap().code, 400);
                assert!(object.actions.is_none());
            }
        }
        for request in requests {
            assert!(matches!(
                lfs_upload_object(&service, &request, vec![]).await,
                Err(GitLFSError::GeneralError(message)) if message.starts_with("Invalid")
            ));
            if request.size >= 0 {
                assert!(matches!(
                    lfs_download_object(service.clone(), request.oid).await,
                    Err(GitLFSError::GeneralError(message)) if message.starts_with("Invalid")
                ));
            }
        }
    }

    #[tokio::test]
    async fn basic_cannot_read_or_overwrite_a_private_key_registered_in_legacy_metadata() {
        let (_dir, service) = lfs_fixture().await;
        let private_key = "media-v1/known-scope/chunks/known-hash";
        let key = lfs_object_key(private_key);
        let original = b"private chunk".to_vec();
        service
            .obj_storage
            .inner
            .put_stream(&key, original.clone().into_stream(), ObjectMeta::default())
            .await
            .unwrap();
        service
            .lfs_storage
            .new_lfs_object(callisto::lfs_objects::Model {
                oid: private_key.into(),
                size: original.len() as i64,
                exist: true,
            })
            .await
            .unwrap();

        let request = RequestObject {
            oid: private_key.into(),
            size: original.len() as i64,
            ..Default::default()
        };
        assert!(
            lfs_upload_object(&service, &request, b"replacement".to_vec())
                .await
                .is_err()
        );
        assert!(
            lfs_download_object(service.clone(), private_key.into())
                .await
                .is_err()
        );
        let batch = lfs_process_batch(
            &service,
            batch_request(Operation::Download, vec![request]),
            "http://localhost",
        )
        .await
        .unwrap();
        assert_eq!(batch.objects[0].error.as_ref().unwrap().code, 400);
        assert!(batch.objects[0].actions.is_none());

        let (mut stream, _) = service.obj_storage.inner.get_stream(&key).await.unwrap();
        let mut actual = Vec::new();
        while let Some(bytes) = stream.next().await {
            actual.extend_from_slice(&bytes.unwrap());
        }
        assert_eq!(actual, original);
    }

    #[tokio::test]
    async fn basic_metadata_failures_are_errors_instead_of_panics() {
        let (_dir, service) = lfs_fixture().await;
        service
            .lfs_storage
            .get_connection()
            .execute_unprepared(
                "CREATE TRIGGER reject_lfs_insert BEFORE INSERT ON lfs_objects BEGIN SELECT RAISE(FAIL, 'test insert failure'); END",
            )
            .await
            .unwrap();
        let request = || RequestObject {
            oid: "a".repeat(64),
            size: 0,
            ..Default::default()
        };
        let result = lfs_process_batch(
            &service,
            batch_request(Operation::Upload, vec![request()]),
            "http://localhost",
        )
        .await;
        assert!(matches!(
            result,
            Err(GitLFSError::GeneralError(message)) if message == "LFS metadata storage operation failed"
        ));

        service
            .lfs_storage
            .get_connection()
            .execute_unprepared("DROP TABLE lfs_objects")
            .await
            .unwrap();
        assert!(matches!(
            lfs_download_object(service.clone(), request().oid).await,
            Err(GitLFSError::GeneralError(message)) if message == "LFS metadata storage operation failed"
        ));
        assert!(matches!(
            lfs_upload_object(&service, &request(), vec![]).await,
            Err(GitLFSError::GeneralError(message)) if message == "LFS metadata storage operation failed"
        ));
    }

    #[tokio::test]
    async fn basic_valid_sha256_objects_keep_upload_and_download_actions() {
        let (_dir, service) = lfs_fixture().await;
        for (oid, data) in [
            (
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                b"hello".as_slice(),
            ),
            (
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                b"".as_slice(),
            ),
        ] {
            let request = || RequestObject {
                oid: oid.into(),
                size: data.len() as i64,
                ..Default::default()
            };
            let upload = lfs_process_batch(
                &service,
                batch_request(Operation::Upload, vec![request()]),
                "http://localhost",
            )
            .await
            .unwrap();
            assert!(
                upload.objects[0]
                    .actions
                    .as_ref()
                    .unwrap()
                    .contains_key(&Action::Upload)
            );
            lfs_upload_object(&service, &request(), data.to_vec())
                .await
                .unwrap();
            let download = lfs_process_batch(
                &service,
                batch_request(Operation::Download, vec![request()]),
                "http://localhost",
            )
            .await
            .unwrap();
            assert!(
                download.objects[0]
                    .actions
                    .as_ref()
                    .unwrap()
                    .contains_key(&Action::Download)
            );
            let mut stream = Box::pin(
                lfs_download_object(service.clone(), oid.into())
                    .await
                    .unwrap(),
            );
            let mut actual = Vec::new();
            while let Some(bytes) = stream.next().await {
                actual.extend_from_slice(&bytes.unwrap());
            }
            assert_eq!(actual, data);
        }
    }

    fn lock(id: &str) -> Lock {
        Lock {
            id: id.to_string(),
            path: format!("/dir/{id}.bin"),
            owner: None,
            locked_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn lock_limit_slices_page_and_reports_next_cursor() {
        let locks = vec![lock("1"), lock("2"), lock("3"), lock("4")];

        let (page, next) = apply_lock_limit(locks, "2").unwrap();
        assert_eq!(
            page.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["1", "2"]
        );
        assert_eq!(next, "3");
    }

    #[test]
    fn lock_limit_beyond_list_returns_everything_without_cursor() {
        let locks = vec![lock("1"), lock("2")];

        let (page, next) = apply_lock_limit(locks, "10").unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(next, "");
    }

    #[test]
    fn empty_limit_returns_unpaged_locks() {
        let locks = vec![lock("1")];

        let (page, next) = apply_lock_limit(locks, "").unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(next, "");
    }

    #[test]
    fn non_numeric_and_negative_limits_are_rejected() {
        // The router classifies by message content ("Invalid..." -> 400), so the
        // rejection must carry that prefix to survive the lookup-error masking
        // in `lfs_retrieve_lock`.
        for limit in ["abc", "-1"] {
            match apply_lock_limit(vec![lock("1")], limit) {
                Err(GitLFSError::GeneralError(msg)) => {
                    assert!(msg.starts_with("Invalid"), "unexpected message: {msg}");
                }
                other => panic!("expected GeneralError, got {other:?}"),
            }
        }
    }

    #[test]
    fn response_object_download_existing() {
        let meta = MetaObject {
            oid: "oid1".into(),
            size: 10,
            exist: true,
        };
        let res = ResponseObject::new(
            &meta,
            ResCondition {
                file_exist: true,
                operation: Operation::Download,
                use_tus: false,
            },
            "http://dl",
            "http://ul",
        );
        assert!(res.actions.is_some());
        let actions = res.actions.unwrap();
        assert!(actions.contains_key(&Action::Download));
        assert!(res.error.is_none());
    }

    #[test]
    fn response_object_upload_new() {
        let meta = MetaObject {
            oid: "oid2".into(),
            size: 20,
            exist: false,
        };
        let res = ResponseObject::new(
            &meta,
            ResCondition {
                file_exist: false,
                operation: Operation::Upload,
                use_tus: false,
            },
            "http://dl",
            "http://ul",
        );
        let actions = res.actions.expect("upload should provide actions");
        assert!(actions.contains_key(&Action::Upload));
        assert!(res.error.is_none());
    }

    #[test]
    fn response_object_download_missing_sets_error() {
        let meta = MetaObject {
            oid: "oid3".into(),
            size: 30,
            exist: false,
        };
        let res = ResponseObject::new(
            &meta,
            ResCondition {
                file_exist: false,
                operation: Operation::Download,
                use_tus: false,
            },
            "http://dl",
            "http://ul",
        );
        assert!(res.actions.is_none());
        assert!(res.error.is_some());
        assert_eq!(res.error.unwrap().code, 404);
    }

    #[test]
    fn unlock_request_defaults() {
        let req = UnlockRequest::default();
        assert!(req.force.is_none());
        assert_eq!(req.refs, Ref { name: "".into() });
    }
}
