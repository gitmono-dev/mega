//! Version-one source identities shared with the Mega snapshot contract.
//!
//! The JSON form is a wire representation, not the bytes to hash. IDs use the
//! explicitly framed encoding below; moving refs and leases are not identity.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError(pub &'static str);

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for IdentityError {}

macro_rules! validated_string {
    ($name:ident, $validator:ident, $message:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                if !$validator(&value) {
                    return Err(IdentityError($message));
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = IdentityError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl FromStr for $name {
            type Err = IdentityError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

fn valid_source(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .map(|id| !id.is_nil() && id.to_string() == value)
        .unwrap_or(false)
}

fn lowercase_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_oid(value: &str) -> bool {
    lowercase_hex(value, 40)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| lowercase_hex(hex, 64))
}

/// Limits are byte lengths, matching the v1 Linux projection contract.
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_COMPONENT_BYTES: usize = 255;

fn valid_components(value: &str) -> bool {
    value.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part.len() <= MAX_COMPONENT_BYTES
            && !part.contains('\0')
    })
}

fn valid_absolute_path(value: &str) -> bool {
    value == "/"
        || (value.len() <= MAX_PATH_BYTES && value.strip_prefix('/').is_some_and(valid_components))
}

fn valid_relative_path(value: &str) -> bool {
    value.is_empty() || (value.len() <= MAX_PATH_BYTES && valid_components(value))
}

fn valid_ref(value: &str) -> bool {
    (value.starts_with("refs/heads/") || value.starts_with("refs/tags/"))
        && value.len() <= 1024
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && !value
            .bytes()
            .any(|b| b <= b' ' || b == 127 || b"~^:?*[\\".contains(&b))
        && value
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

validated_string!(
    SourceId,
    valid_source,
    "source ID must be a non-nil canonical UUID"
);
validated_string!(
    ObjectId,
    valid_oid,
    "v1 Git object ID must be 40 lowercase hexadecimal digits"
);
validated_string!(
    ManifestDigest,
    valid_digest,
    "digest must be sha256 followed by 64 lowercase hexadecimal digits"
);
validated_string!(
    RepoPath,
    valid_absolute_path,
    "path must be absolute, canonical UTF-8 and within v1 byte limits"
);
validated_string!(
    RelativePath,
    valid_relative_path,
    "relative path must be canonical UTF-8 and within v1 byte limits"
);
validated_string!(
    RefName,
    valid_ref,
    "ref must be a canonical fully qualified branch or tag"
);

impl RepoPath {
    /// Component-aware containment, not a raw starts_with check.
    pub fn relative_to(&self, scope: &RepoPath) -> Option<RelativePath> {
        let relative = if scope.as_str() == "/" {
            self.as_str().strip_prefix('/')?
        } else if self == scope {
            ""
        } else {
            self.as_str()
                .strip_prefix(scope.as_str())?
                .strip_prefix('/')?
        };
        RelativePath::new(relative).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectFormat {
    Sha1,
}

impl ObjectFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
        }
    }
}

/// Structural validity is not a server attestation: the resolver must also
/// prove the commit/tree/scope relationship and enforce authorization.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    pub source_id: SourceId,
    pub scope_path: RepoPath,
    pub object_format: ObjectFormat,
    pub commit_oid: ObjectId,
    pub root_tree_oid: ObjectId,
}

impl SourceSnapshot {
    /// Domain bytes, then five UTF-8 fields with unsigned big-endian u32 lengths.
    /// Field order is part of v1. There are no optional or floating-point fields.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = b"mega.source-snapshot.v1\0".to_vec();
        for field in [
            self.source_id.as_str(),
            self.scope_path.as_str(),
            self.object_format.as_str(),
            self.commit_oid.as_str(),
            self.root_tree_oid.as_str(),
        ] {
            // All fields are validated and bounded well below u32::MAX.
            bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
            bytes.extend_from_slice(field.as_bytes());
        }
        bytes
    }

    /// Provenance identity. This is NOT a namespace view or a projection key:
    /// two commits with the same tree may still share a verified object cache.
    pub fn id(&self) -> ManifestDigest {
        let bytes = self.canonical_bytes();
        let digest = {
            use sha2::Digest;
            hex::encode(sha2::Sha256::digest(&bytes))
        };
        ManifestDigest(format!("sha256:{digest}"))
    }
}

/// Only used before resolving. Immutable readers never retain this selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceSelector {
    SourceCommit {
        source_id: SourceId,
        scope_path: RepoPath,
        commit_oid: ObjectId,
    },
    SourceRef {
        source_id: SourceId,
        scope_path: RepoPath,
        ref_name: RefName,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Vector {
        source: SourceSnapshot,
        canonical_hex: String,
        source_id_digest: ManifestDigest,
    }

    #[test]
    fn source_golden_vectors_match_independent_encoding() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../../tests/fixtures/snapshot/source-v1.json"))
                .unwrap();
        for vector in vectors {
            assert_eq!(
                hex::encode(vector.source.canonical_bytes()),
                vector.canonical_hex
            );
            assert_eq!(vector.source.id(), vector.source_id_digest);
            let json = serde_json::to_string(&vector.source).unwrap();
            assert_eq!(
                serde_json::from_str::<SourceSnapshot>(&json).unwrap(),
                vector.source
            );
        }
    }

    #[test]
    fn paths_preserve_names_and_enforce_component_boundaries() {
        let scope = RepoPath::new("/project/a").unwrap();
        assert_eq!(
            RepoPath::new("/project/a/src/a+b.rs")
                .unwrap()
                .relative_to(&scope)
                .unwrap()
                .as_str(),
            "src/a+b.rs"
        );
        assert!(
            RepoPath::new("/project/ab")
                .unwrap()
                .relative_to(&scope)
                .is_none()
        );
        assert_eq!(scope.relative_to(&scope).unwrap().as_str(), "");
        let unicode = RepoPath::new("/第三方/e\u{301}").unwrap();
        assert_eq!(unicode.as_str(), "/第三方/e\u{301}");
        for invalid in [
            "",
            "project/a",
            "//a",
            "/a/",
            "/a//b",
            "/a/./b",
            "/a/../b",
            "/a\0b",
        ] {
            assert!(RepoPath::new(invalid).is_err(), "{invalid:?}");
        }
        assert!(RepoPath::new(format!("/{}", "a".repeat(256))).is_err());
        assert!(RelativePath::new("/absolute").is_err());
        assert!(RelativePath::new("../outside").is_err());
    }

    #[test]
    fn deserialization_cannot_bypass_identity_validation() {
        for invalid in [
            "\"BAD\"",
            "\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"",
            "\"0000000000000000000000000000000000000000000000000000000000000000\"",
        ] {
            assert!(serde_json::from_str::<ObjectId>(invalid).is_err());
        }
        assert!(SourceId::new("00000000-0000-0000-0000-000000000000").is_err());
        assert!(SourceId::new("https://example.test/repo").is_err());
        assert!(serde_json::from_str::<ObjectFormat>("\"sha256\"").is_err());
        assert!(ManifestDigest::new(format!("sha256:{}", "A".repeat(64))).is_err());
    }

    #[test]
    fn symbolic_refs_are_typed_and_fully_qualified() {
        for valid in [
            "refs/heads/main",
            "refs/tags/v1.2.3+build",
            "refs/heads/团队/分支",
        ] {
            assert!(RefName::new(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "main",
            "HEAD",
            "refs/cl/123",
            "refs/heads/",
            "refs/heads/a..b",
            "refs/tags/.x",
            "refs/tags/x.lock",
            "refs/heads/a@{b",
            "refs/heads/a//b",
            "refs/heads/a?b",
            "refs/heads/a\\b",
        ] {
            assert!(RefName::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn provenance_includes_scope_source_and_commit_even_when_tree_is_equal() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../../tests/fixtures/snapshot/source-v1.json"))
                .unwrap();
        let source = vectors.into_iter().next().unwrap().source;
        let mut other = source.clone();
        other.commit_oid = ObjectId::new("2".repeat(40)).unwrap();
        assert_ne!(source.id(), other.id());
        other = source.clone();
        other.scope_path = RepoPath::new("/different").unwrap();
        assert_ne!(source.id(), other.id());
        other = source.clone();
        other.source_id = SourceId::new("22222222-2222-4222-8222-222222222222").unwrap();
        assert_ne!(source.id(), other.id());
    }
}
