//! Persistent, compressed byte-radix index from canonical repository paths to
//! immutable binding digests. No live registry reads, mutable nodes or recursion.
//! Public pagination/authentication belongs above this internal index.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use common::errors::MegaError;
use sha2::{Digest, Sha256};

use crate::model::snapshot::{MAX_PATH_BYTES, ManifestDigest, RepoPath};

pub const NODE_DOMAIN: &[u8] = b"mega.namespace-radix.v1\0";
pub mod database;
pub const MAX_NODE_BYTES: usize = 16 * 1024;
pub const MAX_PAGE_SIZE: usize = 256;

#[async_trait]
pub trait NodeStore: Send + Sync {
    async fn read(&self, digest: &ManifestDigest) -> Result<Vec<u8>, MegaError>;
    /// Insert-only, durable before the enclosing publication transaction commits.
    /// A store must reject the same digest with different bytes, never overwrite.
    async fn write(&self, digest: &ManifestDigest, bytes: &[u8]) -> Result<(), MegaError>;
}

pub fn digest(bytes: &[u8]) -> ManifestDigest {
    ManifestDigest::new(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
        .expect("SHA-256 yields a canonical digest")
}

/// The empty index is a well-known, implicit node; no DB row is required.
pub fn empty_root() -> ManifestDigest {
    digest(&Node::empty().encode())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    label: Vec<u8>,
    value: Option<ManifestDigest>,
    children: BTreeMap<u8, ManifestDigest>,
}

impl Node {
    fn empty() -> Self {
        Self {
            label: Vec::new(),
            value: None,
            children: BTreeMap::new(),
        }
    }
    fn leaf(label: &[u8], value: ManifestDigest) -> Self {
        Self {
            label: label.to_vec(),
            value: Some(value),
            children: BTreeMap::new(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = NODE_DOMAIN.to_vec();
        out.extend_from_slice(&(self.label.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.label);
        out.push(u8::from(self.value.is_some()));
        if let Some(value) = &self.value {
            out.extend_from_slice(&raw_digest(value));
        }
        out.extend_from_slice(&(self.children.len() as u16).to_be_bytes());
        for (edge, child) in &self.children {
            out.push(*edge);
            out.extend_from_slice(&raw_digest(child));
        }
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self, MegaError> {
        if bytes.len() > MAX_NODE_BYTES {
            return Err(corrupt("node byte limit"));
        }
        let mut input = bytes
            .strip_prefix(NODE_DOMAIN)
            .ok_or_else(|| corrupt("node schema"))?;
        let label_len = read_u16(&mut input)?;
        if label_len > MAX_PATH_BYTES {
            return Err(corrupt("label byte limit"));
        }
        let label = take(&mut input, label_len)?.to_vec();
        let value = match take(&mut input, 1)?[0] {
            0 => None,
            1 => Some(read_digest(&mut input)?),
            _ => return Err(corrupt("value tag")),
        };
        let count = read_u16(&mut input)?;
        if count > 256 {
            return Err(corrupt("fanout limit"));
        }
        let mut children = BTreeMap::new();
        let mut previous = None;
        for _ in 0..count {
            let edge = take(&mut input, 1)?[0];
            if previous.is_some_and(|old| edge <= old) {
                return Err(corrupt("unsorted or duplicate edge"));
            }
            children.insert(edge, read_digest(&mut input)?);
            previous = Some(edge);
        }
        if !input.is_empty()
            || (value.is_none() && count == 1)
            || (value.is_none() && count == 0 && !label.is_empty())
        {
            return Err(corrupt("noncanonical node"));
        }
        Ok(Self {
            label,
            value,
            children,
        })
    }
}

fn raw_digest(value: &ManifestDigest) -> Vec<u8> {
    hex::decode(&value.as_str()[7..]).expect("validated digest")
}
fn read_digest(input: &mut &[u8]) -> Result<ManifestDigest, MegaError> {
    Ok(ManifestDigest::new(format!("sha256:{}", hex::encode(take(input, 32)?))).expect("32 bytes"))
}
fn read_u16(input: &mut &[u8]) -> Result<usize, MegaError> {
    let bytes = take(input, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}
fn take<'a>(input: &mut &'a [u8], count: usize) -> Result<&'a [u8], MegaError> {
    if input.len() < count {
        return Err(corrupt("truncated node"));
    }
    let (head, tail) = input.split_at(count);
    *input = tail;
    Ok(head)
}
fn corrupt(detail: &str) -> MegaError {
    MegaError::Unavailable(format!("namespace index integrity failure: {detail}"))
}

/// A trailing NUL marks each component. Parent paths are key prefixes but
/// prefix neighbors (/rust and /rust_v1) are never ancestor bindings.
fn key(path: &RepoPath) -> Vec<u8> {
    if path.as_str() == "/" {
        return Vec::new();
    }
    let mut bytes = path.as_str().as_bytes()[1..].to_vec();
    for byte in &mut bytes {
        if *byte == b'/' {
            *byte = 0;
        }
    }
    bytes.push(0);
    bytes
}
fn path_from_key(key: &[u8]) -> Result<RepoPath, MegaError> {
    if key.is_empty() {
        return Ok(RepoPath::new("/").expect("root"));
    }
    if key.last() != Some(&0) {
        return Err(corrupt("value outside component boundary"));
    }
    let mut bytes = vec![b'/'];
    bytes.extend(
        key[..key.len() - 1]
            .iter()
            .map(|b| if *b == 0 { b'/' } else { *b }),
    );
    let text = String::from_utf8(bytes).map_err(|_| corrupt("non-UTF-8 key"))?;
    RepoPath::new(text).map_err(|_| corrupt("invalid key path"))
}

pub struct RadixIndex<'a> {
    store: &'a dyn NodeStore,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IndexPage {
    pub entries: Vec<(RepoPath, ManifestDigest)>,
    pub has_more: bool,
}

impl<'a> RadixIndex<'a> {
    pub fn new(store: &'a dyn NodeStore) -> Self {
        Self { store }
    }

    async fn load(&self, id: &ManifestDigest, edge: Option<u8>) -> Result<Node, MegaError> {
        let node = if id == &empty_root() {
            Node::empty()
        } else {
            let bytes = self.store.read(id).await?;
            if bytes.len() > MAX_NODE_BYTES {
                return Err(corrupt("node byte limit"));
            }
            if digest(&bytes) != *id {
                return Err(corrupt("node digest"));
            }
            Node::decode(&bytes)?
        };
        if edge.is_some_and(|edge| node.label.first() != Some(&edge)) {
            return Err(corrupt("child label does not match edge"));
        }
        Ok(node)
    }

    async fn save(&self, mut node: Node) -> Result<Option<ManifestDigest>, MegaError> {
        if node.value.is_none() {
            if node.children.is_empty() {
                return Ok(None);
            }
            if node.children.len() == 1 {
                let (edge, id) = node.children.first_key_value().expect("one child");
                let child = self.load(id, Some(*edge)).await?;
                node.label.extend_from_slice(&child.label);
                node.value = child.value;
                node.children = child.children;
            }
        }
        if node.label.len() > MAX_PATH_BYTES {
            return Err(corrupt("combined label limit"));
        }
        let bytes = node.encode();
        if bytes.len() > MAX_NODE_BYTES {
            return Err(corrupt("encoded node limit"));
        }
        let id = digest(&bytes);
        self.store.write(&id, &bytes).await?;
        Ok(Some(id))
    }

    /// Copy only changed ancestors; a no-op returns the same root without writes.
    /// The old root remains valid. Deletion canonicalizes compressed edges, so
    /// insert order and a delete/reinsert roundtrip cannot change the digest.
    pub async fn update(
        &self,
        root: &ManifestDigest,
        path: &RepoPath,
        value: Option<ManifestDigest>,
    ) -> Result<ManifestDigest, MegaError> {
        let key = key(path);
        let mut offset = 0;
        let mut id = root.clone();
        let mut edge = None;
        let mut ancestors = Vec::new();
        let mut replacement;
        loop {
            let mut node = self.load(&id, edge).await?;
            let remaining = &key[offset..];
            let common = remaining
                .iter()
                .zip(&node.label)
                .take_while(|(a, b)| a == b)
                .count();
            if common < node.label.len() {
                let Some(value) = value else {
                    return Ok(root.clone());
                };
                let mut parent = Node {
                    label: node.label[..common].to_vec(),
                    ..Node::empty()
                };
                node.label.drain(..common);
                parent.children.insert(
                    node.label[0],
                    self.save(node).await?.expect("existing nonempty node"),
                );
                if common == remaining.len() {
                    parent.value = Some(value);
                } else {
                    let suffix = &remaining[common..];
                    parent.children.insert(
                        suffix[0],
                        self.save(Node::leaf(suffix, value)).await?.expect("leaf"),
                    );
                }
                replacement = self.save(parent).await?;
                break;
            }
            offset += common;
            if offset == key.len() {
                if node.value == value {
                    return Ok(root.clone());
                }
                node.value = value;
                replacement = self.save(node).await?;
                break;
            }
            let next_edge = key[offset];
            if let Some(child) = node.children.get(&next_edge) {
                id = child.clone();
                edge = Some(next_edge);
                ancestors.push((node, next_edge));
            } else {
                let Some(value) = value else {
                    return Ok(root.clone());
                };
                node.children.insert(
                    next_edge,
                    self.save(Node::leaf(&key[offset..], value))
                        .await?
                        .expect("leaf"),
                );
                replacement = self.save(node).await?;
                break;
            }
        }
        while let Some((mut parent, edge)) = ancestors.pop() {
            match replacement {
                Some(id) => {
                    parent.children.insert(edge, id);
                }
                None => {
                    parent.children.remove(&edge);
                }
            }
            replacement = self.save(parent).await?;
        }
        Ok(replacement.unwrap_or_else(empty_root))
    }

    pub async fn get(
        &self,
        root: &ManifestDigest,
        path: &RepoPath,
    ) -> Result<Option<ManifestDigest>, MegaError> {
        Ok(self.walk(root, path, false).await?.map(|(_, value)| value))
    }

    pub async fn longest_prefix(
        &self,
        root: &ManifestDigest,
        path: &RepoPath,
    ) -> Result<Option<(RepoPath, ManifestDigest)>, MegaError> {
        self.walk(root, path, true).await
    }

    async fn walk(
        &self,
        root: &ManifestDigest,
        path: &RepoPath,
        ancestors: bool,
    ) -> Result<Option<(RepoPath, ManifestDigest)>, MegaError> {
        let key = key(path);
        let mut id = root.clone();
        let mut edge = None;
        let mut offset = 0;
        let mut found = None;
        loop {
            let node = self.load(&id, edge).await?;
            if !key[offset..].starts_with(&node.label) {
                break;
            }
            offset += node.label.len();
            if let Some(value) = node.value {
                found = Some((path_from_key(&key[..offset])?, value));
            }
            if offset == key.len() {
                return Ok(found.filter(|(p, _)| ancestors || p == path));
            }
            let next_edge = key[offset];
            let Some(child) = node.children.get(&next_edge) else {
                break;
            };
            id = child.clone();
            edge = Some(next_edge);
        }
        Ok(if ancestors { found } else { None })
    }

    /// Internal keyset page in encoded-component order. HTTP must authenticate
    /// its cursor and bind it to view/prefix/query/schema; raw `after` is not an
    /// externally trustworthy cursor. Only intersecting subtrees are visited.
    pub async fn page(
        &self,
        root: &ManifestDigest,
        prefix: &RepoPath,
        after: Option<&RepoPath>,
        limit: usize,
    ) -> Result<IndexPage, MegaError> {
        if limit == 0
            || limit > MAX_PAGE_SIZE
            || after.is_some_and(|p| p.relative_to(prefix).is_none())
        {
            return Err(MegaError::bad_request("invalid namespace index page"));
        }
        let prefix = key(prefix);
        let after = after.map(key);
        let mut stack = vec![(root.clone(), Arc::<[u8]>::from([]), None)];
        let mut entries = Vec::new();
        while let Some((id, parent, edge)) = stack.pop() {
            let node = self.load(&id, edge).await?;
            let full: Arc<[u8]> = [parent.as_ref(), &node.label].concat().into();
            if full.len() > MAX_PATH_BYTES {
                return Err(corrupt("key byte limit"));
            }
            if !intersects(&full, &prefix, after.as_deref()) {
                continue;
            }
            if let Some(value) = node.value
                && full.starts_with(&prefix)
                && after
                    .as_ref()
                    .is_none_or(|after| full.as_ref() > after.as_slice())
            {
                entries.push((path_from_key(&full)?, value));
                if entries.len() > limit {
                    entries.pop();
                    return Ok(IndexPage {
                        entries,
                        has_more: true,
                    });
                }
            }
            for (edge, child) in node.children.into_iter().rev() {
                let mut lower_bound = full.to_vec();
                lower_bound.push(edge);
                if intersects(&lower_bound, &prefix, after.as_deref()) {
                    stack.push((child, full.clone(), Some(edge)));
                }
            }
        }
        Ok(IndexPage {
            entries,
            has_more: false,
        })
    }
}

fn intersects(candidate: &[u8], prefix: &[u8], after: Option<&[u8]>) -> bool {
    (candidate.starts_with(prefix) || prefix.starts_with(candidate))
        && !after.is_some_and(|after| candidate < after && !after.starts_with(candidate))
}

#[cfg(test)]
mod tests;
