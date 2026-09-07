# Namespace manifest identity v1

Status: shared codec implemented and tested, 2026-09-06. This is a content
identity contract, not a claim of publication, authorization, leases or FUSE
integration. Both repositories consume the same `namespace-v1.json` fixture;
the committed PowerShell 7 generator independently frames bytes and hashes with
.NET, without calling the Rust implementation.

## Binding

JSON has exactly `mount_path`, `source_snapshot`, `source_subpath` and `policy`.
The first is a canonical RepoPath; source_subpath is a canonical RelativePath
relative to the attested source scope. Their source-side composition must still
fit the 4096-byte absolute-path limit. Tree existence, source membership,
ancestor/descendant binding conflicts and release enforcement are publisher
checks, not proofs supplied by this structural codec.

Canonical bytes are ASCII `mega.namespace-binding.v1` plus NUL, followed by:

1. Mount path: u32 big-endian byte length, then UTF-8 bytes.
2. Full canonical SourceSnapshot bytes from source-snapshot-v1, framed by u32
   big-endian byte length. This is not JSON and not merely the source UUID.
3. Source subpath: u32 big-endian byte length, then UTF-8 bytes.
4. Policy u8: 1 = `mutable`, 2 = `immutable_release`.

Policy is explicit and part of identity; it is never guessed from a numeric
directory name. **D2 is confirmed:** an explicitly marked release directory
cannot change content after its first publication; ordinary development
bindings may evolve. A codec that can encode both values does not itself enforce
this rule on writers.

## View

JSON has exactly `schema_version`, `instance_id`, `native`, `bindings_root`,
`overrides_root` and `materialization_policy`. Schema version must be integer 1.
Instance ID is a distinct non-nil canonical UUID type, not a source ID.
The native SourceSnapshot must have root scope `/`; the server must separately
attest that its source is the instance's native backend.

Canonical bytes are ASCII `mega.namespace-view.v1` plus NUL, followed by:

1. Schema version: u16 big-endian, exactly 1.
2. Instance UUID: u32 big-endian byte length, then canonical lowercase UUID text.
3. Full canonical native SourceSnapshot bytes, framed by u32 big-endian length.
4. Bindings root: 32 raw SHA-256 bytes, with no textual prefix.
5. Overrides presence: u8 0 for absent, or u8 1 followed by 32 raw digest bytes.
6. Materialization policy u8: 1 = `git_raw_v1`.

`git_raw_v1` identifies raw Git projection without implicit LFS hydration or
submodule expansion. It is not permission to traverse arbitrary external
symlinks. Overrides are representable in the codec; the reader must explicitly
reject that capability until the override route semantics are implemented.
The absent root is distinct from a present empty-index root.

No timestamp, actor, operation ID, publication sequence, parent view, floating
ref, lease or client generation is hashed into a view. A different commit with
the same tree changes provenance and therefore changes view_id. Re-publishing
identical content can reuse view_id while publication metadata remains separate.

## Strict decoding and cross-repository evidence

Every complete manifest is limited to 16384 bytes. Hash identity is `sha256:`
plus lowercase hex SHA-256 of the entire domain-separated canonical byte
sequence. Binary decoding rejects truncation, length overflow, trailing bytes,
unknown schema/policy/optional tags, invalid UTF-8 or paths and mismatched
domains. JSON rejects unknown fields and passes through the same structural
validation as constructors. Decoding bytes is not digest verification against a
requested ID; the content-store/read boundary must do that separately.

Golden view without overrides:
`sha256:3c8632afb308bf562973b3af517ae5d0a27c05651f3f7511f91e16d7ad8f1231`.

Golden mutable binding:
`sha256:adebe124b05761074c9460ed20426acf3023645e2bfa7e46b12239da68b14a88`.

Both repositories pass five codec tests: independent binary/JSON vectors,
identity changes with provenance/routing/instance/policy, JSON rejection,
all-prefix truncation and malformed tags/lengths, and maximum-length paths.
Mega uses SHA-2 and ScorpioFS uses ring, while the oracle uses .NET SHA-256.
This closes the shared manifest-identity subtask, not the full G01–G06/V01–V18
acceptance suite.
