use std::{collections::HashSet, str::FromStr};

use common::errors::MegaError;
use git_internal::{
    hash::{HashKind, ObjectHash},
    internal::object::{ObjectTrait, tree::Tree},
};
use jupiter::storage::git_db_storage::GitDbStorage;
use sha1::{Digest, Sha1};

/// A resolved import root never consults a moving ref again.
/// This internal value does not claim a namespace publication or a retention lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedImportCommit {
    pub repo_id: i64,
    pub commit_oid: String,
    pub root_tree_oid: String,
}

/// Compatibility selector for existing code browsing APIs: absent/empty means
/// default, a full SHA-1 means commit, a fully qualified ref is exact, and an
/// unqualified name means a legacy tag (never an ambiguous branch fallback).
pub(crate) async fn resolve_import_commit(
    storage: &GitDbStorage,
    repo_id: i64,
    reference: Option<&str>,
) -> Result<ResolvedImportCommit, MegaError> {
    let reference = reference.unwrap_or_default().trim();
    let (mut oid, peel_tags) = if reference.is_empty() {
        let default_ref = storage
            .get_unique_default_ref(repo_id)
            .await?
            .ok_or_else(|| MegaError::NotFound("default import ref not found".into()))?;
        (default_ref.ref_git_id, false)
    } else if reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit()) {
        (reference.to_ascii_lowercase(), false)
    } else {
        let full_ref =
            if reference.starts_with("refs/heads/") || reference.starts_with("refs/tags/") {
                reference.to_owned()
            } else if reference.starts_with("refs/") {
                return Err(MegaError::bad_request("unsupported import ref namespace"));
            } else {
                format!("refs/tags/{reference}")
            };
        let selected = storage
            .get_ref_by_name(repo_id, &full_ref)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("import ref not found: {full_ref}")))?;
        (selected.ref_git_id, full_ref.starts_with("refs/tags/"))
    };

    let mut seen = HashSet::new();
    // The final commit after 32 annotated tags is accepted; a 33rd tag is not.
    for depth in 0..=32 {
        if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(MegaError::Unavailable(
                "invalid stored import object id".into(),
            ));
        }
        oid.make_ascii_lowercase();
        if !seen.insert(oid.clone()) {
            return Err(MegaError::bad_request("cyclic annotated tag chain"));
        }
        if let Some(commit) = storage.get_commit_by_hash(repo_id, &oid).await? {
            let tree_oid = ObjectHash::from_str(&commit.tree)
                .map_err(|_| MegaError::Unavailable("invalid stored import root tree id".into()))?;
            if tree_oid.kind() != HashKind::Sha1 {
                return Err(MegaError::Unavailable(
                    "import commit and tree hash formats differ".into(),
                ));
            }
            return Ok(ResolvedImportCommit {
                repo_id,
                commit_oid: commit.commit_id,
                root_tree_oid: commit.tree,
            });
        }
        if !peel_tags {
            return Err(MegaError::NotFound(format!(
                "import commit not found: {oid}"
            )));
        }
        let tag = storage
            .get_tag_by_hash(repo_id, &oid)
            .await?
            .ok_or_else(|| {
                MegaError::NotFound(format!("import tag target is not a commit or tag: {oid}"))
            })?;
        if tag.object_type != "commit" && tag.object_type != "tag" {
            return Err(MegaError::bad_request("tag does not resolve to a commit"));
        }
        if depth == 32 {
            return Err(MegaError::bad_request(
                "annotated tag chain exceeds 32 objects",
            ));
        }
        oid = tag.object_id;
    }
    unreachable!("the bounded tag loop returns at its depth limit")
}

pub(crate) async fn read_import_root(
    storage: &GitDbStorage,
    source: &ResolvedImportCommit,
) -> Result<Tree, MegaError> {
    let model = storage
        .get_tree_by_hash(source.repo_id, &source.root_tree_oid)
        .await?
        .ok_or_else(|| {
            MegaError::NotFound(format!("import tree not found: {}", source.root_tree_oid))
        })?;
    let oid = ObjectHash::from_str(&model.tree_id)
        .map_err(|_| MegaError::Unavailable("invalid stored import tree id".into()))?;
    if oid.kind() != HashKind::Sha1 {
        return Err(MegaError::Unavailable(
            "unsupported import tree hash format".into(),
        ));
    }
    // ObjectHash::new selects a thread-local algorithm, not the algorithm of
    // this async request. Hash this SHA-1 source explicitly, including Git's header.
    let mut hash = Sha1::new();
    hash.update(format!("tree {}\0", model.sub_trees.len()).as_bytes());
    hash.update(&model.sub_trees);
    if hex::encode(hash.finalize()) != source.root_tree_oid {
        return Err(MegaError::Unavailable(
            "stored import tree hash mismatch".into(),
        ));
    }
    Tree::from_bytes(&model.sub_trees, oid)
        .map_err(|e| MegaError::Unavailable(format!("invalid stored import tree: {e}")))
}

#[cfg(test)]
mod tests;
