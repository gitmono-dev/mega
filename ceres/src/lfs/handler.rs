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

///
///
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
                        .unwrap();
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
    Ok(storage.get_lfs_object(oid).await.unwrap().map(|m| m.into()))
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
    use super::*;
    use crate::lfs::lfs_structs::{Action, Ref, ResCondition, ResponseObject};

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
