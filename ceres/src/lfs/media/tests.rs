use std::sync::Arc;

use common::config::{LocalConfig, ObjectStorageBackend, ObjectStorageConfig};
use io_orbit::factory::ObjectStorageFactory;
use jupiter::storage::{
    base_storage::{BaseStorage, StorageConnector},
    lfs_db_storage::LfsDbStorage,
};
use sea_orm::{ConnectionTrait, Database};

use super::{
    protocol::{ChunkEntry, CreatedBy},
    *,
};

async fn fixture() -> (tempfile::TempDir, LfsService, MediaScope) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE lfs_objects (oid TEXT PRIMARY KEY, size BIGINT NOT NULL, exist BOOLEAN NOT NULL)").await.unwrap();
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
    (
        dir,
        service,
        MediaScope::new("alice", "/project/demo.git").unwrap(),
    )
}

fn sample(data: &[u8]) -> MediaManifest {
    MediaManifest {
        version: 1,
        algorithm: chunker::ALGORITHM.into(),
        hash_algorithm: "sha256".into(),
        media_oid: sha256_hex(data),
        media_size: data.len() as u64,
        chunks: chunker::chunk_bytes(data)
            .into_iter()
            .map(|c| ChunkEntry {
                offset: c.offset,
                length: c.length,
                chunk_hash: c.chunk_hash,
                encoded_length: c.length,
                compression: "none".into(),
                checksum: None,
            })
            .collect(),
        created_by: CreatedBy {
            client: "libra".into(),
            version: "fixture".into(),
            capabilities: vec![chunker::ALGORITHM.into()],
        },
        fallback_oid: None,
    }
}

async fn upload_all(
    service: &LfsService,
    scope: &MediaScope,
    manifest: &MediaManifest,
    data: &[u8],
) -> String {
    let prepared = prepare(service, scope, manifest.clone()).await.unwrap();
    for hash in prepared.missing_chunks {
        let c = manifest
            .chunks
            .iter()
            .find(|c| c.chunk_hash == hash)
            .unwrap();
        upload_chunk(
            service,
            scope,
            &prepared.manifest_id,
            &hash,
            data[c.offset as usize..(c.offset + c.length) as usize].to_vec(),
        )
        .await
        .unwrap();
    }
    prepared.manifest_id
}

#[tokio::test]
async fn roundtrip_resume_dedup_isolation_and_standard_fallback() {
    let (_dir, service, scope) = fixture().await;
    let mut seed = 0x1234_5678_9abc_def0u64;
    let data: Vec<u8> = (0..12 * 1024 * 1024)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u8
        })
        .collect();
    let manifest = sample(&data);
    assert!(manifest.chunks.len() > 1);
    assert!(
        manifest
            .chunks
            .windows(2)
            .any(|c| c[0].length != c[1].length)
    );
    let first = prepare(&service, &scope, manifest.clone()).await.unwrap();
    assert!(matches!(
        get_manifest(&service, &scope, &manifest.media_oid).await,
        Err(MediaError::NotFound)
    ));
    assert!(
        finalize(&service, &scope, &first.manifest_id)
            .await
            .is_err()
    );
    let chunk = &manifest.chunks[0];
    assert!(
        upload_chunk(
            &service,
            &scope,
            &first.manifest_id,
            &chunk.chunk_hash,
            vec![0; chunk.length as usize]
        )
        .await
        .is_err()
    );
    upload_chunk(
        &service,
        &scope,
        &first.manifest_id,
        &chunk.chunk_hash,
        data[..chunk.length as usize].to_vec(),
    )
    .await
    .unwrap();
    let resumed = prepare(&service, &scope, manifest.clone()).await.unwrap();
    assert_eq!(resumed.manifest_id, first.manifest_id);
    assert!(!resumed.missing_chunks.contains(&chunk.chunk_hash));
    assert!(matches!(
        download_chunk(&service, &scope, &manifest.media_oid, &chunk.chunk_hash).await,
        Err(MediaError::NotFound)
    ));
    let id = upload_all(&service, &scope, &manifest, &data).await;
    finalize(&service, &scope, &id).await.unwrap();
    finalize(&service, &scope, &id).await.unwrap();
    assert!(
        prepare(&service, &scope, manifest.clone())
            .await
            .unwrap()
            .missing_chunks
            .is_empty()
    );
    assert_eq!(
        get_manifest(&service, &scope, &manifest.media_oid)
            .await
            .unwrap()
            .manifest_id,
        id
    );
    let stream =
        crate::lfs::handler::lfs_download_object(service.clone(), manifest.media_oid.clone())
            .await
            .unwrap();
    let mut stream = Box::pin(stream);
    let mut whole = Vec::new();
    while let Some(bytes) = stream.next().await {
        whole.extend_from_slice(&bytes.unwrap());
    }
    assert_eq!(whole, data);
    for other in [
        MediaScope::new("bob", "/project/demo.git").unwrap(),
        MediaScope::new("alice", "/project/other.git").unwrap(),
    ] {
        assert!(matches!(
            get_manifest(&service, &other, &manifest.media_oid).await,
            Err(MediaError::NotFound)
        ));
        assert!(matches!(
            upload_chunk(&service, &other, &id, &chunk.chunk_hash, vec![]).await,
            Err(MediaError::NotFound)
        ));
        assert_eq!(
            prepare(&service, &other, manifest.clone())
                .await
                .unwrap()
                .missing_chunks
                .len(),
            manifest.chunks.len()
        );
    }
    // A new file revision reuses unchanged content-defined chunks, not merely
    // a repeated upload of the exact same object.
    let mut edited = data.clone();
    edited[0] ^= 1;
    let changed = sample(&edited);
    let delta = prepare(&service, &scope, changed.clone()).await.unwrap();
    assert_ne!(changed.media_oid, manifest.media_oid);
    assert_eq!(
        delta.missing_chunks,
        vec![changed.chunks[0].chunk_hash.clone()]
    );
    let changed_id = upload_all(&service, &scope, &changed, &edited).await;
    finalize(&service, &scope, &changed_id).await.unwrap();
    assert_eq!(
        get_manifest(&service, &scope, &manifest.media_oid)
            .await
            .unwrap()
            .manifest_id,
        id
    );
}

#[tokio::test]
async fn rejects_wrong_media_hash_and_noncanonical_chunking_without_publication() {
    let (_dir, service, scope) = fixture().await;
    let data = b"hello world";
    let mut manifest = sample(data);
    manifest.media_oid = "a".repeat(64);
    let id = upload_all(&service, &scope, &manifest, data).await;
    assert!(finalize(&service, &scope, &id).await.is_err());
    assert!(matches!(
        get_manifest(&service, &scope, &manifest.media_oid).await,
        Err(MediaError::NotFound)
    ));
    let mut manifest = sample(data);
    manifest.chunks = [(&data[..5], 0), (&data[5..], 5)]
        .into_iter()
        .map(|(bytes, offset)| ChunkEntry {
            offset,
            length: bytes.len() as u64,
            encoded_length: bytes.len() as u64,
            chunk_hash: sha256_hex(bytes),
            compression: "none".into(),
            checksum: None,
        })
        .collect();
    let id = upload_all(&service, &scope, &manifest, data).await;
    assert!(finalize(&service, &scope, &id).await.is_err());
    assert!(matches!(
        get_manifest(&service, &scope, &manifest.media_oid).await,
        Err(MediaError::NotFound)
    ));
}

#[tokio::test]
async fn empty_object_roundtrip_and_expired_pending_rejected() {
    let (_dir, service, scope) = fixture().await;
    let manifest = sample(&[]);
    let id = upload_all(&service, &scope, &manifest, &[]).await;
    finalize(&service, &scope, &id).await.unwrap();
    assert!(
        get_manifest(&service, &scope, &manifest.media_oid)
            .await
            .unwrap()
            .manifest
            .chunks
            .is_empty()
    );
    let expired = Pending {
        created_at: 0,
        manifest,
    };
    put(
        &service,
        &scope.key(&format!("pending/{id}")),
        serde_json::to_vec(&expired).unwrap(),
    )
    .await
    .unwrap();
    assert!(matches!(
        finalize(&service, &scope, &id).await,
        Err(MediaError::NotFound)
    ));
}

#[test]
fn rejects_malformed_manifests_and_scope_paths() {
    for repo in ["", "relative", "/../secret", "/a//b", "/%2fsecret", "/a\\b"] {
        assert!(MediaScope::new("alice", repo).is_err());
    }
    assert!(MediaScope::new("", "/repo").is_err());
    let original = sample(b"hello");
    for change in 0..7 {
        let mut m = original.clone();
        match change {
            0 => m.chunks[0].length = 0,
            1 => m.chunks[0].length = u64::MAX,
            2 => m.chunks[0].offset = 1,
            3 => m.chunks[0].encoded_length = 0,
            4 => m.chunks[0].chunk_hash = "../secret".into(),
            5 => m.fallback_oid = Some("b".repeat(64)),
            _ => m.chunks[0].compression = "gzip".into(),
        }
        assert!(m.validate().is_err());
    }
}
