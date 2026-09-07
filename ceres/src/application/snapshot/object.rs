//! Strict SHA-1 object decoding for snapshot reads. Do not use the legacy Git
//! parser's thread-local algorithm or non-UTF-8 name conversion on this boundary.

use std::collections::HashSet;

use common::errors::MegaError;
use sha1::{Digest, Sha1};

use crate::model::snapshot::{MAX_COMPONENT_BYTES, ObjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Tree,
    Blob,
}

impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Blob => "blob",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Executable,
    Symlink,
    Gitlink,
}

impl EntryKind {
    pub fn object_kind(self) -> Result<ObjectKind, MegaError> {
        match self {
            Self::Directory => Ok(ObjectKind::Tree),
            Self::File | Self::Executable | Self::Symlink => Ok(ObjectKind::Blob),
            Self::Gitlink => Err(MegaError::bad_request(
                "snapshot submodule hydration is unsupported",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedEntry {
    pub name: String,
    pub kind: EntryKind,
    pub oid: ObjectId,
}

pub fn verify_object(kind: ObjectKind, oid: &ObjectId, payload: &[u8]) -> Result<(), MegaError> {
    let mut hash = Sha1::new();
    hash.update(format!("{} {}\0", kind.as_str(), payload.len()).as_bytes());
    hash.update(payload);
    if hex::encode(hash.finalize()) != oid.as_str() {
        return Err(MegaError::Unavailable(
            "snapshot object integrity failure".into(),
        ));
    }
    Ok(())
}

pub fn decode_tree(mut raw: &[u8]) -> Result<Vec<FixedEntry>, MegaError> {
    let malformed = || MegaError::Unavailable("malformed or unsupported snapshot tree".into());
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    while !raw.is_empty() {
        let space = raw.iter().position(|b| *b == b' ').ok_or_else(malformed)?;
        let kind = match &raw[..space] {
            b"40000" | b"040000" => EntryKind::Directory,
            b"100644" => EntryKind::File,
            b"100755" => EntryKind::Executable,
            b"120000" => EntryKind::Symlink,
            b"160000" => EntryKind::Gitlink,
            _ => return Err(malformed()),
        };
        raw = &raw[space + 1..];
        let nul = raw.iter().position(|b| *b == 0).ok_or_else(malformed)?;
        let name = std::str::from_utf8(&raw[..nul]).map_err(|_| malformed())?;
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.len() > MAX_COMPONENT_BYTES
            || !names.insert(name.to_owned())
        {
            return Err(malformed());
        }
        raw = &raw[nul + 1..];
        let oid = raw.get(..20).ok_or_else(malformed)?;
        entries.push(FixedEntry {
            name: name.to_owned(),
            kind,
            oid: ObjectId::new(hex::encode(oid)).map_err(|_| malformed())?,
        });
        raw = &raw[20..];
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mode: &[u8], name: &[u8]) -> Vec<u8> {
        [mode, b" ", name, b"\0", &[0x11; 20]].concat()
    }

    #[test]
    fn strict_decoder_rejects_ambiguous_names_modes_and_truncation() {
        for bad in [
            entry(b"100644", b".."),
            entry(b"100644", b"a/b"),
            entry(b"100644", &[0xff]),
            entry(b"100600", b"x"),
            [entry(b"100644", b"x"), entry(b"100755", b"x")].concat(),
            b"100644 x\0short".to_vec(),
            b"100644".to_vec(),
        ] {
            assert!(decode_tree(&bad).is_err());
        }
        assert!(decode_tree(&[]).unwrap().is_empty());
        assert_eq!(
            decode_tree(&entry(b"120000", b"link")).unwrap()[0].kind,
            EntryKind::Symlink
        );
        assert_eq!(
            decode_tree(&entry(b"160000", b"sub")).unwrap()[0].kind,
            EntryKind::Gitlink
        );
    }

    #[test]
    fn verification_uses_git_type_and_length_header() {
        let _guard =
            git_internal::hash::set_hash_kind_for_test(git_internal::hash::HashKind::Sha256);
        let empty = ObjectId::new("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap();
        verify_object(ObjectKind::Tree, &empty, &[]).unwrap();
        assert!(verify_object(ObjectKind::Blob, &empty, &[]).is_err());
        assert!(verify_object(ObjectKind::Tree, &empty, b"changed").is_err());
    }
}
