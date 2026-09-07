//! Immutable namespace identity codec. Structural validity is not publication,
//! source authorization, scope attestation or an object-retention lease.

use serde::{Deserialize, Serialize};

use super::snapshot::{
    IdentityError, MAX_PATH_BYTES, ManifestDigest, ObjectFormat, ObjectId, RelativePath, RepoPath,
    SourceId, SourceSnapshot,
};

pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const BINDING_DOMAIN: &[u8] = b"mega.namespace-binding.v1\0";
const VIEW_DOMAIN: &[u8] = b"mega.namespace-view.v1\0";

/// Distinct from a source UUID even though both use the same UUID syntax.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(SourceId);

impl InstanceId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        SourceId::new(value).map(Self)
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Explicit data, never inferred from a numeric directory name. Encoding both
/// policies does not choose the deployment's release-directory policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingPolicy {
    Mutable,
    ImmutableRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationPolicy {
    GitRawV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BindingFields", into = "BindingFields")]
pub struct NamespaceBinding(BindingFields);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFields {
    mount_path: RepoPath,
    source_snapshot: SourceSnapshot,
    source_subpath: RelativePath,
    policy: BindingPolicy,
}

impl TryFrom<BindingFields> for NamespaceBinding {
    type Error = IdentityError;
    fn try_from(fields: BindingFields) -> Result<Self, Self::Error> {
        // A binding may expose a subtree of an attested source. It must still
        // have a representable absolute source path for membership proofs.
        let scope = fields.source_snapshot.scope_path.as_str();
        let subpath = fields.source_subpath.as_str();
        let length = scope.len() + usize::from(scope != "/" && !subpath.is_empty()) + subpath.len();
        if length > MAX_PATH_BYTES {
            return Err(IdentityError("binding source path exceeds v1 limit"));
        }
        Ok(Self(fields))
    }
}
impl From<NamespaceBinding> for BindingFields {
    fn from(value: NamespaceBinding) -> Self {
        value.0
    }
}

impl NamespaceBinding {
    pub fn new(
        mount_path: RepoPath,
        source_snapshot: SourceSnapshot,
        source_subpath: RelativePath,
        policy: BindingPolicy,
    ) -> Result<Self, IdentityError> {
        BindingFields {
            mount_path,
            source_snapshot,
            source_subpath,
            policy,
        }
        .try_into()
    }
    pub fn mount_path(&self) -> &RepoPath {
        &self.0.mount_path
    }
    pub fn source_snapshot(&self) -> &SourceSnapshot {
        &self.0.source_snapshot
    }
    pub fn source_subpath(&self) -> &RelativePath {
        &self.0.source_subpath
    }
    pub fn policy(&self) -> BindingPolicy {
        self.0.policy
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = BINDING_DOMAIN.to_vec();
        frame(&mut out, self.mount_path().as_str().as_bytes());
        frame(&mut out, &self.source_snapshot().canonical_bytes());
        frame(&mut out, self.source_subpath().as_str().as_bytes());
        out.push(match self.policy() {
            BindingPolicy::Mutable => 1,
            BindingPolicy::ImmutableRelease => 2,
        });
        out
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        let mut r = Reader::new(bytes, BINDING_DOMAIN)?;
        let mount = RepoPath::new(r.text()?)?;
        let source = read_source(r.field()?)?;
        let subpath = RelativePath::new(r.text()?)?;
        let policy = match r.byte()? {
            1 => BindingPolicy::Mutable,
            2 => BindingPolicy::ImmutableRelease,
            _ => return Err(IdentityError("unknown binding policy")),
        };
        r.finish()?;
        Self::new(mount, source, subpath, policy)
    }

    pub fn id(&self) -> ManifestDigest {
        hash_bytes(&self.canonical_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ViewFields", into = "ViewFields")]
pub struct NamespaceView(ViewFields);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewFields {
    schema_version: u16,
    instance_id: InstanceId,
    native: SourceSnapshot,
    bindings_root: ManifestDigest,
    overrides_root: Option<ManifestDigest>,
    materialization_policy: MaterializationPolicy,
}

impl TryFrom<ViewFields> for NamespaceView {
    type Error = IdentityError;
    fn try_from(fields: ViewFields) -> Result<Self, Self::Error> {
        if fields.schema_version != 1 {
            return Err(IdentityError("unknown namespace view schema"));
        }
        if fields.native.scope_path.as_str() != "/" {
            return Err(IdentityError(
                "namespace native snapshot must cover root scope",
            ));
        }
        Ok(Self(fields))
    }
}
impl From<NamespaceView> for ViewFields {
    fn from(value: NamespaceView) -> Self {
        value.0
    }
}

impl NamespaceView {
    pub fn new(
        instance_id: InstanceId,
        native: SourceSnapshot,
        bindings_root: ManifestDigest,
        overrides_root: Option<ManifestDigest>,
        materialization_policy: MaterializationPolicy,
    ) -> Result<Self, IdentityError> {
        ViewFields {
            schema_version: 1,
            instance_id,
            native,
            bindings_root,
            overrides_root,
            materialization_policy,
        }
        .try_into()
    }
    pub fn instance_id(&self) -> &InstanceId {
        &self.0.instance_id
    }
    pub fn native(&self) -> &SourceSnapshot {
        &self.0.native
    }
    pub fn bindings_root(&self) -> &ManifestDigest {
        &self.0.bindings_root
    }
    pub fn overrides_root(&self) -> Option<&ManifestDigest> {
        self.0.overrides_root.as_ref()
    }
    pub fn materialization_policy(&self) -> MaterializationPolicy {
        self.0.materialization_policy
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = VIEW_DOMAIN.to_vec();
        out.extend_from_slice(&1u16.to_be_bytes());
        frame(&mut out, self.instance_id().as_str().as_bytes());
        frame(&mut out, &self.native().canonical_bytes());
        out.extend_from_slice(&raw_digest(self.bindings_root()));
        match self.overrides_root() {
            None => out.push(0),
            Some(root) => {
                out.push(1);
                out.extend_from_slice(&raw_digest(root));
            }
        }
        out.push(match self.materialization_policy() {
            MaterializationPolicy::GitRawV1 => 1,
        });
        out
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        let mut r = Reader::new(bytes, VIEW_DOMAIN)?;
        if r.take(2)? != [0, 1] {
            return Err(IdentityError("unknown namespace view schema"));
        }
        let instance = InstanceId::new(r.text()?)?;
        let native = read_source(r.field()?)?;
        let bindings = r.digest()?;
        let overrides = match r.byte()? {
            0 => None,
            1 => Some(r.digest()?),
            _ => return Err(IdentityError("invalid overrides presence tag")),
        };
        let policy = match r.byte()? {
            1 => MaterializationPolicy::GitRawV1,
            _ => return Err(IdentityError("unknown materialization policy")),
        };
        r.finish()?;
        Self::new(instance, native, bindings, overrides, policy)
    }

    pub fn id(&self) -> ManifestDigest {
        hash_bytes(&self.canonical_bytes())
    }
}

fn frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}
fn raw_digest(digest: &ManifestDigest) -> Vec<u8> {
    hex::decode(&digest.as_str()[7..]).expect("validated digest")
}
fn hash_bytes(bytes: &[u8]) -> ManifestDigest {
    use sha2::Digest;
    let hex_digest = hex::encode(sha2::Sha256::digest(bytes));
    ManifestDigest::new(format!("sha256:{hex_digest}")).expect("SHA-256 digest")
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], domain: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(IdentityError("namespace manifest exceeds byte limit"));
        }
        bytes
            .strip_prefix(domain)
            .map(Self)
            .ok_or(IdentityError("invalid manifest domain"))
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8], IdentityError> {
        if len > self.0.len() {
            return Err(IdentityError("truncated manifest"));
        }
        let (head, tail) = self.0.split_at(len);
        self.0 = tail;
        Ok(head)
    }
    fn byte(&mut self) -> Result<u8, IdentityError> {
        Ok(self.take(1)?[0])
    }
    fn field(&mut self) -> Result<&'a [u8], IdentityError> {
        let size = u32::from_be_bytes(self.take(4)?.try_into().expect("four bytes")) as usize;
        self.take(size)
    }
    fn text(&mut self) -> Result<&'a str, IdentityError> {
        std::str::from_utf8(self.field()?).map_err(|_| IdentityError("non-UTF-8 manifest field"))
    }
    fn digest(&mut self) -> Result<ManifestDigest, IdentityError> {
        ManifestDigest::new(format!("sha256:{}", hex::encode(self.take(32)?)))
    }
    fn finish(self) -> Result<(), IdentityError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(IdentityError("trailing manifest bytes"))
        }
    }
}
fn read_source(bytes: &[u8]) -> Result<SourceSnapshot, IdentityError> {
    let mut r = Reader::new(bytes, b"mega.source-snapshot.v1\0")?;
    let source_id = SourceId::new(r.text()?)?;
    let scope_path = RepoPath::new(r.text()?)?;
    let object_format = match r.text()? {
        "sha1" => ObjectFormat::Sha1,
        _ => return Err(IdentityError("unknown source object format")),
    };
    let commit_oid = ObjectId::new(r.text()?)?;
    let root_tree_oid = ObjectId::new(r.text()?)?;
    r.finish()?;
    Ok(SourceSnapshot {
        source_id,
        scope_path,
        object_format,
        commit_oid,
        root_tree_oid,
    })
}

#[cfg(test)]
mod tests;
