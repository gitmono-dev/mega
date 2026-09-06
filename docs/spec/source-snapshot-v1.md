# Source snapshot v1 contract

Status: implemented identity/read foundations, 2026-09-06. This contract does not advertise a deployed snapshot capability. Namespace publication, leases, scope attestation coverage and FUSE integration remain separate gates in the versioning specs.

## JSON and validation

The source descriptor has exactly these five fields:

```json
{
  "source_id": "11111111-1111-4111-8111-111111111111",
  "scope_path": "/project/a",
  "object_format": "sha1",
  "commit_oid": "1111111111111111111111111111111111111111",
  "root_tree_oid": "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
}

```

The hashes above illustrate structure, not a deployed source or a claimed commit/tree relationship.

- source_id is a non-nil, lowercase, hyphenated UUID persisted by Mega. Its server-side mapping includes instance, backend kind and repo ID. Paths are not source IDs; recreating a different logical source must not reuse an ID.
- scope_path is canonical absolute UTF-8. / is the root; other paths cannot end in / or contain empty, dot or parent components or NUL. Limit: 4096 UTF-8 bytes per protocol path, 255 per component. This protocol limit is not a guarantee that any host-local mountpoint prefix fits an OS path limit.
- Names retain case, Unicode composition, plus signs and literal backslashes. No Windows path normalization is applied. Non-UTF-8 names are unsupported in v1.
- object_format is sha1. Object IDs contain exactly 40 lowercase hexadecimal digits. Future algorithms require negotiation, not automatic acceptance.
- Unknown fields, invalid IDs and unknown algorithm tags fail deserialization. Structural validation does not prove scope, object membership or commit.tree: Mega must attest those relationships.

The source_ref selector requires a fully qualified refs/heads/... or refs/tags/... name. The source_commit selector accepts only a commit OID, never an arbitrary tree/tag OID. The compatibility parser in existing Mega browsing APIs still accepts an unqualified tag name; the new typed contract does not.

## Canonical source identity

Do not hash a JSON serialization. Canonical source bytes consist of:

1. The ASCII domain mega.source-snapshot.v1 followed by one NUL byte.
2. The five fields in this order: source_id, scope_path, object_format, commit_oid, root_tree_oid.
3. Each field is encoded as its unsigned 32-bit big-endian UTF-8 byte length followed immediately by those UTF-8 bytes. OIDs are lowercase hex text here, not raw 20-byte values.

source identity = sha256: followed by the lowercase SHA-256 hex digest of those bytes.

The same shared fixture is tested by both implementations. It includes an ASCII scope and a Unicode/plus-sign scope. The first vector's identity is sha256:6e3f8a7e41d3a9759bc05cbc1dab153ad27ba0e0ff494f7692392dbfd5a95451. Fixture bytes/digests were independently computed with .NET; Ceres uses RustCrypto SHA-256 and ScorpioFS uses ring.

This identity includes commit provenance. It is not namespace view_id, publication_seq, a lease, or a projection_key. A same-tree/different-commit pair has different source identities but may still share verified physical objects within an authorized domain.

## Immutable object boundary

ScorpioFS SourceReader owns a fixed SourceSnapshot and only asks ObjectBackend for (source, object kind, OID, root-relative source_path, byte limit). It exposes no mutable ref selector. A different version requires another reader; a caller cannot mutate the descriptor held by an existing reader.

source_path is a membership/authorization context, not a lookup through current routing: the server must walk from the descriptor's fixed root and verify the resulting kind/OID before returning an object. Root uses the empty relative path. This avoids a whole-repository reachability scan per request. Signed object tickets may optimize the same check later; arbitrary caller-supplied OIDs are never sufficient proof.

Backends must check current authorization and retention even on a global CAS hit, enforce limits during download, and return raw object payloads. The client verifies SHA-1 over Git's type + space + decimal length + NUL + payload. A file beginning with Git-like header bytes retains those bytes.

Tree traversal is relative to the source root. Prefix neighbors such as /project/ab do not match scope /project/a. A scope commit's tree is already rooted at the scope; the prefix is never applied twice. Tree names, entry modes, symlink targets, missing paths and failed object fetches remain distinct.

The initial reader is a bounded whole-object implementation: default limits are 16 MiB/tree and 64 MiB/blob. It returns an explicit size-limit error, never empty bytes, on oversized objects. This is not the final streaming/CAS/FUSE adapter or a claim that stat is metadata-only. Namespace routing, chunked large-object reads and controlled workspace generation changes are not completed by these tests.

## Mega source observations and scope proofs

SourceCatalog registers stable backend IDs and resolves typed selectors. A new import observation uses its registered root and a repo-scoped commit/tag resolution. A new native observation requires an exact scoped ref whose stored root agrees with commit.tree; it records native_ref_observed, not a claim that an older writer emitted a creation proof. Native projection derives a child by walking an already attested fixed root and preserves the base commit provenance.

Explicit native commits without a proof for the requested scope return SCOPE_UNKNOWN. A recorded descriptor can be resolved after refs or registry entries are removed; a reused path assigned to a different repo ID receives a different source ID. No current registry lookup is used to read an already attested source.

The catalog checks descriptor attestation and walks root-relative paths to bind object kind/OID to source membership. It strictly decodes UTF-8 trees and checks SHA-1 independent of git-internal's thread-local algorithm. It is an internal metadata service, not an authorization or retention grant. Public HTTP reads must add those checks, and no snapshot endpoint or capability is enabled by the catalog alone. Observing individual sources is not an atomic multi-source namespace publication. Existing commit metadata is trusted ingestion state; this foundation does not claim raw commit/tag payload re-verification or complete proof capture by every writer.

## Verification

Run the relevant repository command:

```sh
cargo test -p ceres --lib snapshot --locked
cargo test --lib snapshot --locked

```

The ScorpioFS reader fixtures obtain their OIDs from git hash-object --stdin (without -w). Git must be installed, but these tests need no network, FUSE mount, mutable global Git configuration or existing repository objects.
