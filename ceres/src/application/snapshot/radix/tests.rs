use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::*;

#[derive(Default)]
struct MemoryStore {
    nodes: Mutex<HashMap<ManifestDigest, Vec<u8>>>,
    reads: AtomicUsize,
    writes: AtomicUsize,
    read_bytes: AtomicUsize,
    write_bytes: AtomicUsize,
    largest: AtomicUsize,
}

#[async_trait]
impl NodeStore for MemoryStore {
    async fn read(&self, id: &ManifestDigest) -> Result<Vec<u8>, MegaError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let bytes = self
            .nodes
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| corrupt("missing node"))?;
        self.read_bytes.fetch_add(bytes.len(), Ordering::Relaxed);
        Ok(bytes)
    }
    async fn write(&self, id: &ManifestDigest, bytes: &[u8]) -> Result<(), MegaError> {
        assert_eq!(&digest(bytes), id);
        assert!(bytes.len() <= MAX_NODE_BYTES);
        let mut nodes = self.nodes.lock().unwrap();
        if let Some(old) = nodes.get(id) {
            assert_eq!(old, bytes);
        }
        nodes.insert(id.clone(), bytes.to_vec());
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.write_bytes.fetch_add(bytes.len(), Ordering::Relaxed);
        self.largest.fetch_max(bytes.len(), Ordering::Relaxed);
        Ok(())
    }
}

impl MemoryStore {
    fn reset_metrics(&self) {
        for metric in [
            &self.reads,
            &self.writes,
            &self.read_bytes,
            &self.write_bytes,
            &self.largest,
        ] {
            metric.store(0, Ordering::Relaxed);
        }
    }
}
fn p(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}
fn v(value: &str) -> ManifestDigest {
    digest(value.as_bytes())
}

#[test]
fn canonical_vectors_match_independent_dotnet_encoding() {
    #[derive(serde::Deserialize)]
    struct Vector {
        name: String,
        canonical_hex: String,
        digest: ManifestDigest,
    }
    let vectors: Vec<Vector> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/snapshot/namespace-radix-v1.json"
    ))
    .unwrap();
    for vector in vectors {
        let bytes = hex::decode(vector.canonical_hex).unwrap();
        assert_eq!(digest(&bytes), vector.digest, "{}", vector.name);
        assert_eq!(Node::decode(&bytes).unwrap().encode(), bytes);
        if vector.name == "empty" {
            assert_eq!(empty_root(), vector.digest);
        }
    }
}

#[tokio::test]
async fn index_is_canonical_independent_of_insert_order_and_preserves_old_roots() {
    let store = MemoryStore::default();
    let index = RadixIndex::new(&store);
    let paths = [
        "/",
        "/third-party/rust",
        "/third-party/rust_v1",
        "/third-party/rust/crate",
        "/project/库+1",
        "/project/ab",
        "/project/a",
    ];
    let mut root = empty_root();
    for path in paths {
        root = index.update(&root, &p(path), Some(v(path))).await.unwrap();
    }
    let mut reverse = empty_root();
    for path in paths.into_iter().rev() {
        reverse = index
            .update(&reverse, &p(path), Some(v(path)))
            .await
            .unwrap();
    }
    assert_eq!(root, reverse);
    let changed = index
        .update(&root, &p(paths[1]), Some(v("new")))
        .await
        .unwrap();
    assert_ne!(root, changed);
    for path in paths {
        assert_eq!(index.get(&root, &p(path)).await.unwrap(), Some(v(path)));
    }
    assert_eq!(
        index.get(&changed, &p(paths[1])).await.unwrap(),
        Some(v("new"))
    );
    let restored = index
        .update(&changed, &p(paths[1]), Some(v(paths[1])))
        .await
        .unwrap();
    assert_eq!(restored, root);
    for path in paths {
        reverse = index.update(&reverse, &p(path), None).await.unwrap();
    }
    assert_eq!(reverse, empty_root());
    store.reset_metrics();
    assert_eq!(
        index.update(&root, &p("/absent"), None).await.unwrap(),
        root
    );
    assert_eq!(
        index
            .update(&root, &p(paths[1]), Some(v(paths[1])))
            .await
            .unwrap(),
        root
    );
    assert_eq!(store.writes.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn pages_stay_on_the_requested_immutable_root_and_component_prefix() {
    let store = MemoryStore::default();
    let index = RadixIndex::new(&store);
    let mut root = empty_root();
    let paths = [
        "/rust",
        "/rust/crate",
        "/rust/a",
        "/rust/a/b",
        "/rust_v1",
        "/rusty/a",
    ];
    for path in paths {
        root = index.update(&root, &p(path), Some(v(path))).await.unwrap();
    }
    assert_eq!(
        index
            .longest_prefix(&root, &p("/rust/crate/src/lib.rs"))
            .await
            .unwrap(),
        Some((p("/rust/crate"), v("/rust/crate")))
    );
    assert_eq!(
        index
            .longest_prefix(&root, &p("/rustz/crate"))
            .await
            .unwrap(),
        None
    );
    assert_eq!(index.get(&root, &p("/rust/crate/src")).await.unwrap(), None);
    let newer = index
        .update(&root, &p("/rust/new"), Some(v("new")))
        .await
        .unwrap();
    let first = index.page(&root, &p("/rust"), None, 2).await.unwrap();
    assert!(first.has_more);
    let after = &first.entries.last().unwrap().0;
    let second = index
        .page(&root, &p("/rust"), Some(after), 2)
        .await
        .unwrap();
    assert!(!second.has_more);
    assert_eq!(
        first
            .entries
            .into_iter()
            .chain(second.entries)
            .map(|(p, _)| p)
            .collect::<Vec<_>>(),
        vec![p("/rust"), p("/rust/a"), p("/rust/a/b"), p("/rust/crate")]
    );
    assert_eq!(
        index
            .page(&newer, &p("/rust"), None, 10)
            .await
            .unwrap()
            .entries
            .len(),
        5
    );
    assert!(
        index
            .page(&root, &p("/rust"), Some(&p("/rust_v1")), 10)
            .await
            .is_err()
    );
    assert!(index.page(&root, &p("/"), None, 0).await.is_err());
    assert!(
        index
            .page(&root, &p("/"), None, MAX_PAGE_SIZE + 1)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn deterministic_mutation_trace_matches_an_independent_ordered_map() {
    let store = MemoryStore::default();
    let index = RadixIndex::new(&store);
    let mut root = empty_root();
    let mut oracle = BTreeMap::new();
    let mut seed = 0x1234u64;
    for step in 0..800 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let path = p(&format!("/pkg/{:03}/crate", (seed >> 32) % 90));
        let value = (!seed.is_multiple_of(5)).then(|| v(&format!("{step}")));
        root = index.update(&root, &path, value.clone()).await.unwrap();
        match value {
            Some(value) => {
                oracle.insert(path.clone(), value);
            }
            None => {
                oracle.remove(&path);
            }
        }
        assert_eq!(
            index.get(&root, &path).await.unwrap(),
            oracle.get(&path).cloned()
        );
        if step % 25 == 0 {
            let page = index.page(&root, &p("/pkg"), None, 100).await.unwrap();
            assert!(!page.has_more);
            assert_eq!(
                page.entries,
                oracle
                    .iter()
                    .map(|(p, v)| (p.clone(), v.clone()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[tokio::test]
async fn corruption_missing_nodes_and_wrong_edge_labels_are_not_empty_successes() {
    let store = MemoryStore::default();
    let index = RadixIndex::new(&store);
    let root = index
        .update(&empty_root(), &p("/a"), Some(v("A")))
        .await
        .unwrap();
    store
        .nodes
        .lock()
        .unwrap()
        .insert(root.clone(), b"bad data".to_vec());
    assert!(index.get(&root, &p("/a")).await.is_err());
    assert!(index.get(&v("absent"), &p("/a")).await.is_err());
    let child = index
        .save(Node::leaf(b"b\0", v("B")))
        .await
        .unwrap()
        .unwrap();
    let bytes = Node {
        label: Vec::new(),
        value: Some(v("root")),
        children: BTreeMap::from([(b'a', child)]),
    }
    .encode();
    let wrong = digest(&bytes);
    store.write(&wrong, &bytes).await.unwrap();
    assert!(index.get(&wrong, &p("/a")).await.is_err());
}

#[test]
fn node_codec_enforces_size_fanout_sorting_and_canonical_compression() {
    let node = Node {
        label: vec![b'a'; MAX_PATH_BYTES],
        value: Some(v("value")),
        children: (0..=255)
            .map(|edge| (edge, v(&format!("{edge}"))))
            .collect(),
    };
    let bytes = node.encode();
    assert!(bytes.len() <= MAX_NODE_BYTES);
    assert_eq!(Node::decode(&bytes).unwrap(), node);
    assert!(Node::decode(&vec![0; MAX_NODE_BYTES + 1]).is_err());
    assert!(Node::decode(&bytes[..bytes.len() - 1]).is_err());
    let single = Node {
        label: b"a".to_vec(),
        value: None,
        children: BTreeMap::from([(b'b', v("b"))]),
    };
    assert!(Node::decode(&single.encode()).is_err());
    let mut trailing = Node::empty().encode();
    trailing.push(0);
    assert!(Node::decode(&trailing).is_err());
    let mut bad_count = Node::empty().encode();
    let len = bad_count.len();
    bad_count[len - 2..].copy_from_slice(&257u16.to_be_bytes());
    assert!(Node::decode(&bad_count).is_err());
}

#[tokio::test]
async fn maximum_path_and_long_shared_prefix_do_not_recurse_or_rewrite_siblings() {
    let store = MemoryStore::default();
    let index = RadixIndex::new(&store);
    let prefix = format!("/{}/", vec!["a".repeat(255); 15].join("/"));
    let path = p(&format!("{prefix}{}", "b".repeat(255)));
    assert_eq!(path.as_str().len(), MAX_PATH_BYTES);
    let first = index
        .update(&empty_root(), &path, Some(v("old")))
        .await
        .unwrap();
    let neighbor = p(&format!("{prefix}{}c", "b".repeat(254)));
    let root = index
        .update(&first, &neighbor, Some(v("neighbor")))
        .await
        .unwrap();
    store.reset_metrics();
    let changed = index.update(&root, &path, Some(v("new"))).await.unwrap();
    assert!(store.writes.load(Ordering::Relaxed) <= 3);
    assert!(store.largest.load(Ordering::Relaxed) <= MAX_NODE_BYTES);
    assert_eq!(index.get(&root, &path).await.unwrap(), Some(v("old")));
    assert_eq!(
        index.get(&changed, &neighbor).await.unwrap(),
        Some(v("neighbor"))
    );
}

// Independent structured fixture generation avoids retaining a million
// intermediate publication roots during initial construction. It does not call
// update/get/page to determine expected mappings; decimal keys are the oracle.
fn decimal_fixture(store: &MemoryStore, digits: usize) -> ManifestDigest {
    fn build(
        nodes: &mut HashMap<ManifestDigest, Vec<u8>>,
        depth: usize,
        digits: usize,
        number: usize,
        label: Vec<u8>,
    ) -> ManifestDigest {
        let node = if depth == digits {
            let mut label = label;
            label.push(0);
            Node::leaf(&label, v(&format!("binding-{number}")))
        } else {
            Node {
                label,
                value: None,
                children: (0..10)
                    .map(|n| {
                        let edge = b'0' + n as u8;
                        (
                            edge,
                            build(nodes, depth + 1, digits, number * 10 + n, vec![edge]),
                        )
                    })
                    .collect(),
            }
        };
        let bytes = node.encode();
        let id = digest(&bytes);
        nodes.insert(id.clone(), bytes);
        id
    }
    build(
        &mut store.nodes.lock().unwrap(),
        0,
        digits,
        0,
        b"third-party\0r".to_vec(),
    )
}

async fn scale_test(digits: usize) {
    let store = MemoryStore::default();
    let root = decimal_fixture(&store, digits);
    let index = RadixIndex::new(&store);
    let number = 10usize.pow(digits as u32) / 2 + 17;
    let path = p(&format!("/third-party/r{number:0digits$}"));
    store.reset_metrics();
    let updated = index
        .update(&root, &path, Some(v("updated")))
        .await
        .unwrap();
    let update_reads = store.reads.load(Ordering::Relaxed);
    let update_writes = store.writes.load(Ordering::Relaxed);
    let read_bytes = store.read_bytes.load(Ordering::Relaxed);
    let write_bytes = store.write_bytes.load(Ordering::Relaxed);
    let largest_node = store.largest.load(Ordering::Relaxed);
    assert!(update_reads <= digits + 2 && update_writes <= digits + 2);
    assert_eq!(
        index.get(&root, &path).await.unwrap(),
        Some(v(&format!("binding-{number}")))
    );
    assert_eq!(
        index.get(&updated, &path).await.unwrap(),
        Some(v("updated"))
    );
    store.reset_metrics();
    let page = index
        .page(&root, &p("/third-party"), Some(&path), 32)
        .await
        .unwrap();
    assert!(page.has_more);
    let page_reads = store.reads.load(Ordering::Relaxed);
    assert!(page_reads < 100);
    for (offset, (path, value)) in page.entries.iter().enumerate() {
        let n = number + offset + 1;
        assert_eq!(path, &p(&format!("/third-party/r{n:0digits$}")));
        assert_eq!(value, &v(&format!("binding-{n}")));
    }
    store.reset_metrics();
    let one = index.page(&root, &path, None, 1).await.unwrap();
    assert_eq!(one.entries.len(), 1);
    assert!(!one.has_more);
    let prefix_reads = store.reads.load(Ordering::Relaxed);
    assert!(prefix_reads <= digits + 2);
    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(str::to_owned)
        });
    println!(
        "bindings={} update_reads={update_reads} update_writes={update_writes} read_bytes={read_bytes} write_bytes={write_bytes} largest_node={largest_node} page32_reads={page_reads} single_prefix_reads={prefix_reads} process_peak={peak:?}",
        10usize.pow(digits as u32)
    );
}

#[tokio::test]
async fn ten_thousand_bindings_keep_updates_and_pages_bounded() {
    scale_test(4).await;
}

#[tokio::test]
#[ignore = "explicit scale gate: cargo test -p ceres --lib snapshot::radix::tests::million -- --ignored --nocapture"]
async fn million_bindings_keep_updates_and_pages_bounded() {
    scale_test(6).await;
}
