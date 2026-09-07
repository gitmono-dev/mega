# Namespace binding index v1

Status: implemented internal index foundation, 2026-09-06. This document fixes the
radix codec used by `ceres::application::snapshot::radix`; it does not enable a
namespace capability. Binding values, complete view manifests, publication
transactions, authenticated public cursors and retention are separate layers.

## Key and node encoding

Keys are canonical `RepoPath` values (UTF-8, at most 4096 bytes, each component at
most 255 bytes). `/` encodes as zero bytes. For any other path, remove the leading
slash, replace each remaining slash with NUL, and append NUL. Thus `/a/b` becomes
`61 00 62 00`; `/rust` is not an ancestor of `/rust_v1`. No Unicode normalization
or case folding occurs. Ordering is unsigned lexicographic encoded-byte order,
not locale or slash-separated string order. Ancestors precede descendants.

Each node is a compressed label, optional immutable binding digest, and sorted
byte-edge children. Canonical bytes, in order:

| Field | Encoding |
| --- | --- |
| domain | ASCII `mega.namespace-radix.v1` followed by NUL |
| label length + label | u16 big-endian byte length, then that many bytes |
| binding present | u8, exactly 0 or 1 |
| binding digest | 32 raw SHA-256 bytes, only if present |
| child count | u16 big-endian, at most 256 |
| each child | u8 edge followed by 32 raw SHA-256 digest bytes; strictly increasing edges |

A child label includes its incoming edge as its first byte. Labels can split a
UTF-8 sequence; only complete value keys are decoded as repository paths. Each
label and assembled key is bounded by 4096 bytes; the complete node by 16384
bytes. A valueless node with one child is compressed into that child. A valueless
node with no children must have an empty label. Trailing bytes, malformed tags,
unsorted or duplicate children, invalid value paths and wrong incoming labels
are errors. Digests are `sha256:` plus lowercase hex of SHA-256 over the entire
canonical byte sequence, including the domain. Unknown schemas fail closed.

The empty root is an implicit canonical node with empty label, no value and no
children; it requires no stored row:

`sha256:18946486089198dfa8eeb70fa90e04b137c579dc08ae1e6f8bceafc0d35ef677`

Independent .NET SHA-256/framing vectors are committed in
`ceres/tests/fixtures/snapshot/namespace-radix-v1.json`. They include empty, leaf
and branch nodes, with exact bytes and digests. Rust tests decode/re-encode and
hash all vectors. The ScorpioFS client does not yet decode this index.

## Operations and persistence

`update(root, path, value)` copies changed ancestors only; deletion compresses
the result. A no-op writes no nodes. Insertion order and delete/reinsert produce
the same root for the same mapping. Old roots and their nodes are never mutated.
`get` and `longest_prefix` use component-aware routing without registry reads.
The implementation uses iterative traversal, not path-depth recursion.

`page(root, prefix, after, limit)` returns up to 256 bindings in encoded-key
order, plus `has_more`, pruning nonintersecting subtrees. `after` must be inside
the prefix and is exclusive. This is an **internal keyset primitive**, not an
authenticated public cursor. HTTP cursors must additionally bind view, prefix,
query, schema and expiry; passing an arbitrary raw `after` externally is not
that contract. Each call uses the explicit immutable root throughout.

The `NodeStore` boundary verifies size and digest before decoding. The Jupiter
adapter inserts immutable rows using the caller's database transaction. SQL
enforces the payload size; the facade additionally verifies schema, digest and
conflicting existing bytes. An absent or corrupt node is an availability error,
never an empty directory. Metadata retention must eventually trace index child
edges and binding/view references; insert-only storage is not a GC policy.

## Reproducible checks and limits of the evidence

Local WSL debug runs (Rust stable, 2026-09-06) use an independent ordered-map
oracle for mutation traces and fixed structured trees for scale fixtures:

| Bindings | Single update reads/writes | Bytes read/written | Largest accessed node | Page 32 reads | One prefix reads |
| --- | --- | --- | --- | --- | --- |
| 10,000 | 5 / 5 | 1515 / 1515 | 372 B | 42 | 5 |
| 1,000,000 | 7 / 7 | 2235 / 2235 | 372 B | 44 | 7 |

These are logical `NodeStore` calls, not SQL statement counts or HTTP latency.
The million-entry test constructs an in-memory six-digit decimal fixture; it is
not a million-row database publication benchmark. Isolated process peak was
about 356 MiB, including the entire in-memory fixture; the combined concurrent
snapshot suite reached about 413 MiB. Neither is per-request service memory.
Worst-case path depth/fanout differs from this decimal fixture and remains
bounded by the codec/key limits, not by these measured averages.

Run `cargo test -p ceres --lib snapshot::radix::tests::million --locked --
--ignored --nocapture` for the scale gate. SQLite and PostgreSQL tests also
exercise real migrated node tables, transaction rollback, reconnection, stable
source identity, scope proofs and old/new root reads. They do not prove atomic
ref/view publication, public cursor isolation, authorization, leases or FUSE
behavior. MG12 and MG15 are therefore only partially covered; do not mark the
broader namespace acceptance suite complete.
