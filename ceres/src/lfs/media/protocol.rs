//! Wire contract shared with Libra's feature-gated `utils::media` implementation.
use serde::{Deserialize, Serialize};

use super::{MediaError, chunker, sha256_hex};

pub const MAX_MANIFEST_SIZE: usize = 10 * 1024 * 1024;
pub const MAX_CHUNKS: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub offset: u64,
    pub length: u64,
    pub chunk_hash: String,
    pub encoded_length: u64,
    pub compression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedBy {
    pub client: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaManifest {
    pub version: u32,
    pub algorithm: String,
    pub hash_algorithm: String,
    pub media_oid: String,
    pub media_size: u64,
    pub chunks: Vec<ChunkEntry>,
    pub created_by: CreatedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_oid: Option<String>,
}

pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl MediaManifest {
    pub fn validate(&self) -> Result<(), MediaError> {
        if self.version != 1
            || self.algorithm != chunker::ALGORITHM
            || self.hash_algorithm != "sha256"
            || !valid_hash(&self.media_oid)
            || self.chunks.len() > MAX_CHUNKS
            || self
                .fallback_oid
                .as_ref()
                .is_some_and(|oid| oid != &self.media_oid)
        {
            return Err(MediaError::Invalid(
                "unsupported or invalid media manifest".into(),
            ));
        }
        let mut offset = 0u64;
        for chunk in &self.chunks {
            if chunk.offset != offset
                || chunk.length == 0
                || chunk.length > chunker::MAX_SIZE as u64
                || !valid_hash(&chunk.chunk_hash)
                || chunk.compression != "none"
                || chunk.encoded_length != chunk.length
                || chunk.checksum.is_some()
            {
                return Err(MediaError::Invalid(
                    "invalid chunk range, hash or encoding".into(),
                ));
            }
            offset = offset
                .checked_add(chunk.length)
                .ok_or_else(|| MediaError::Invalid("chunk offset overflow".into()))?;
        }
        if offset != self.media_size {
            return Err(MediaError::Invalid(
                "chunk lengths do not match media_size".into(),
            ));
        }
        Ok(())
    }

    /// Content identity excludes client provenance. A verified frozen chunking
    /// of a given media OID has exactly one ID, including across client versions.
    pub fn id(&self) -> Result<String, MediaError> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(
            self.version,
            &self.algorithm,
            &self.hash_algorithm,
            &self.media_oid,
            self.media_size,
            &self.chunks,
        ))?;
        Ok(sha256_hex(&bytes))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareResponse {
    pub manifest_id: String,
    pub missing_chunks: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestResponse {
    pub manifest_id: String,
    pub manifest: MediaManifest,
}

pub fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "version": "1", "chunked_lfs": true,
        "chunk_algorithms": [chunker::ALGORITHM], "hash_algorithms": ["sha256"],
        "max_chunk_size": chunker::MAX_SIZE, "max_manifest_size": MAX_MANIFEST_SIZE,
        "supports_batch_exists": true, "supports_range_read": false,
        "supports_standard_lfs_fallback": true,
        "scope": "authenticated-user-and-repository"
    })
}
