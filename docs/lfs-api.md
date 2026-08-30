# Git LFS API

Git LFS stores large blobs outside the Git object graph. Clients negotiate upload/download through batch requests and use separate object endpoints for binary transfer.

Official references:

- [Batch API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md)
- [Locking API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/locking.md)
- [Server discovery](https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md)

Interactive docs: start `mono` and open Swagger UI at `/swagger-ui` (LFS routes under the LFS tag). See [architecture.md](architecture.md#http-api-discovery).

## URL layout

Mega exposes the same handlers on two prefixes:

| Audience | Base path | Example |
|----------|-----------|---------|
| Git LFS clients | `<repo>.git/info/lfs` | `/project/foo.git/info/lfs/objects/batch` |
| OpenAPI / tools | `/api/v1/lfs` | `/api/v1/lfs/objects/batch` |

Repo paths follow monorepo layout (`/project/...`, `/third-party/...`). The HTTP server rewrites `.../info/lfs/...` request URIs so handlers see a normalized path (`mono/src/server/http_server.rs`).

## Content types

| Use | Content-Type |
|-----|--------------|
| JSON request/response | `application/vnd.git-lfs+json` |
| Object download body | `application/octet-stream` |

## Endpoints

Relative to either base path above:

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/objects/batch` | Batch upload/download negotiation |
| `GET` | `/objects/{oid}` | Download object (binary stream) |
| `PUT` | `/objects/{oid}` | Upload object (binary body) |
| `GET` | `/locks` | List locks (`path`, `cursor`, `limit`, `refspec` query params) |
| `POST` | `/locks` | Create lock |
| `POST` | `/locks/verify` | Verify locks before push |
| `POST` | `/locks/{id}/unlock` | Delete lock |

Chunk download endpoints (`/objects/{oid}/chunks/...`) are **not** exposed in the current router.

## Optional FastCDC transport with Libra

Build both programs with `--features fastcdc` (Mega: `cargo build -p mono
--features fastcdc`; Libra: `cargo build --features fastcdc`). Both features are
off by default. Libra's normal LFS upload/download paths then probe the extension;
`libra config lfs.fastcdc false` disables its use for that repository. Standard
batch requests, LFS pointers and full object URLs retain the `basic` protocol.

The extension uses Libra's **frozen in-tree `fastcdc-v1`** algorithm, not the
third-party `fastcdc::v2020` algorithm: 512 KiB minimum, 2 MiB target, 8 MiB maximum,
fixed SplitMix64 gear table, SHA-256 over raw chunks and the complete file. The
server re-chunks the reconstructed file before finalization to verify boundaries.
See `ceres/src/lfs/media/chunker.rs`, ported from Libra commit `92e1d64a`.

The base is **`<repo>.git/info/lfs/libra/media/v1`**, for example
`/project/demo.git/info/lfs/libra/media/v1`. The URI rewrite retains the original
repository path in a request extension. Scope-less `/api/v1/lfs` and `/info/lfs`
requests cannot access media data. Canonical repository paths are required;
encoded path aliases and dot segments are rejected.

The content-type table above describes standard LFS endpoints. The media
extension uses `application/json` for manifest uploads and successful JSON
responses, and `application/octet-stream` for raw chunk bodies.

All extension endpoints require `Authorization: Bearer <Mono access token>`.
Libra uses its host-scoped stored token, also for capability discovery. Missing
or invalid tokens make discovery fall back to standard LFS. No credentials or
storage object keys are returned in error bodies.

Use a **Mono-issued** token from the server's existing authenticated
`POST /api/v1/user/token/generate` flow. `libra auth login` only saves that token
locally; it neither issues a Mega token nor converts a GitHub PAT or browser
session cookie into one. With Mega listening on localhost port 8000, run the
feature-built Libra binary in a Libra repository:

```bash
libra config remote.origin.url http://localhost:8000/project/demo.git
libra auth login --host http://localhost:8000
# Paste the Mono access token at the hidden prompt.
libra auth status --host http://localhost:8000
libra config lfs.fastcdc true
libra media probe --remote origin
```

The token scope must match the remote's **host and port**. Non-loopback remotes
require HTTPS; HTTP token attachment is allowed only for loopback. `--host` takes
an origin without the repository path. In scripts, supply the token on stdin
using `--with-token`, never as an argv value or inside the remote URL. Compiling
the feature does not replace an independently installed `libra` on PATH.
An unset `lfs.fastcdc` permits negotiation in a feature-enabled build; `true`
explicitly enables it and `false` disables it for that repository. `media probe`
checks remote capabilities, not this repository setting, so a `chunked` result
alone does not confirm that normal LFS transfers will use the extension.

| Method | Relative path | Contract |
|--------|---------------|----------|
| GET | `/capabilities` | Version, frozen algorithm, size limits and fallback support |
| POST | `/manifests` | Validate manifest, persist Pending descriptor, return `manifest_id` and `missing_chunks` (batch existence query) |
| PUT | `/manifests/{id}/chunks/{hash}` | Upload one referenced raw chunk; verify size and SHA-256 |
| POST | `/manifests/{id}/finalize` | Verify chunks, whole hash and CDC boundaries; store full LFS fallback; publish Finalized manifest |
| GET | `/manifests/by-media/{oid}` | Return only a Finalized manifest and its `manifest_id` |
| GET | `/manifests/by-media/{oid}/chunks/{hash}` | Read a chunk referenced by that Finalized object, with integrity verification |

The manifest JSON matches Libra `MediaManifest`: `version`, `algorithm`,
`hash_algorithm`, `media_oid`, `media_size`, `chunks`, `created_by`, `fallback_oid`.
Each chunk has `offset`, `length`, `chunk_hash`, `encoded_length`, `compression`;
v1 only allows `compression: "none"` and omits the reserved `checksum` field.
Chunks must cover the file without gaps/overlaps, have positive bounded lengths,
and use lowercase SHA-256 hashes. Empty files have zero chunks. Maximum manifest
body: 10 MiB; maximum chunk count: 8192. Chunk request bodies are bounded at 8 MiB.

`manifest_id` is SHA-256 of the compact JSON array
`[version,algorithm,hash_algorithm,media_oid,media_size,chunks]`. Client provenance
does not affect identity. Frozen boundary validation means two valid manifests
for the same content have the same ID. Finalized publication is an atomic object
store PUT, after the complete fallback and database metadata are persisted; a
crash before publication leaves no readable manifest. Repeating prepare/upload/
finalize is safe. An interrupted client repeats prepare and uploads only missing
chunks. Pending descriptors expire after 24 hours; repeat prepare to resume later.

Libra caches verified chunks outside the Git object database. Downloads reuse
intact cached chunks, repair corrupted cached chunks, and replace the destination
atomically only after the full SHA-256 verifies. A missing Finalized manifest
uses the complete LFS object instead; authentication/integrity failures after
selecting a manifest are errors, not silent fallback.

### Security and operational boundary

This is an **opt-in transport**, not completion of the entire Lore §6 server plan.
Mega's existing LFS API has no complete repository ACL implementation, so media
storage is conservatively isolated by **authenticated user + repository**. Users
cannot enumerate or download each other's chunks, even when hashes are known.
Every chunk operation also requires a Pending manifest ID or Finalized media OID;
there is no bare global chunk-hash endpoint. Another user's download uses the
standard full-object fallback. The existing standard LFS access policy is unchanged.

Private backend keys are in the dedicated `media` namespace under
`media-v1/<scope-hash>/`, with separate
`pending`, `chunks`, and `finalized` prefixes. Full fallback objects keep their
existing OID keys. Back up both LFS objects and media metadata together. There is
currently **no automatic orphan GC, quota accounting, shared repository ACL,
obliteration, or byte-range hydration**. Expired descriptors and orphan chunks
remain on disk; do not turn on broad production access without retention/quota
policy. Never configure a blanket chunk expiry rule: finalized objects can share
chunks within their scope. Finalization limits concurrent full-file staging to two
operations per server process and uses temporary files. Complete fallback objects
use a bounded multipart writer (8 MiB parts, one in flight); empty objects use an
empty single PUT. Local and cloud storage publish only completed writes. An
S3-compatible backend must support multipart upload for this extension; failures
are returned without falling back to whole-file buffering. Configure the backend's
incomplete-multipart lifecycle cleanup for process crashes.

### Tests and cross-repository verification

```text
# Mega: storage, LFS service and actual HTTP router/authentication tests
cargo test -p io-orbit --lib
cargo test -p ceres --features fastcdc --lib lfs::
cargo test -p mono --features fastcdc --lib api::router::lfs_router::

# Libra: chunker, manifest, cache, fallback and corruption tests
cargo test --features fastcdc --lib utils::media::
cargo test --features fastcdc --test media_fastcdc_test
```

For the two-process interop test, set `MEGA_FASTCDC_READY_FILE` to the **same
absolute temporary file path** in two terminals. Start Mega's isolated server:

```text
cargo test -p mono --features fastcdc --lib serve_libra_interop -- --ignored --nocapture
```

After the ready file appears, run in Libra:

```text
cargo test --features fastcdc --test media_fastcdc_test mega_fastcdc_http_interop -- --ignored --nocapture
```

The fixture uses the production media routes, basic LFS service handlers,
URI rewrite and access-token lookup,
with private temporary storage/SQLite and fixed **test-only** tokens. It listens
only on loopback and stops after ten minutes or `POST /__test/stop`. The test covers
Libra's normal LFS batch/upload/download entry points, interrupted upload replay,
dedup, cached-chunk resume/repair, cross-user isolation and full-object fallback,
empty files, and recovery after fallback persistence but before manifest publication.

## Examples

### Batch (download)

```bash
curl -X POST \
  -H "Content-Type: application/vnd.git-lfs+json" \
  -d '{
    "operation": "download",
    "transfers": ["basic"],
    "objects": [{"oid": "abc123...", "size": 1024}],
    "hash_algo": "sha256"
  }' \
  http://localhost:8000/project/demo.git/info/lfs/objects/batch
```

### Upload object

```bash
curl -X PUT \
  --data-binary @file.bin \
  http://localhost:8000/project/demo.git/info/lfs/objects/abc123...
```

### Download object

```bash
curl -L \
  -H "Accept: application/octet-stream" \
  http://localhost:8000/project/demo.git/info/lfs/objects/abc123... -o file.bin
```

### Lock management

```bash
# List locks
curl "http://localhost:8000/project/demo.git/info/lfs/locks?path=foo.bin&limit=50"

# Create lock
curl -X POST \
  -H "Content-Type: application/vnd.git-lfs+json" \
  -d '{"path":"foo.bin","ref":{"name":"main"}}' \
  http://localhost:8000/project/demo.git/info/lfs/locks

# Delete lock
curl -X POST \
  -H "Content-Type: application/vnd.git-lfs+json" \
  -d '{"force":false,"ref":{"name":"main"}}' \
  http://localhost:8000/project/demo.git/info/lfs/locks/{id}/unlock
```

## Implementation notes

- **Error mapping:** Router maps handler messages to HTTP status — `404` for not found, `400` for invalid input, `500` otherwise (`map_lfs_error` in `mono/src/api/router/lfs_router.rs`).
- **Batch download:** Missing objects should appear as per-object `error` fields with overall HTTP `200`, not a top-level failure.
- **Download Content-Type:** Object downloads return `application/octet-stream`, not LFS JSON.

## Source files

| Layer | Path |
|-------|------|
| Routes | `mono/src/api/router/lfs_router.rs` |
| Business logic | `ceres/src/lfs/handler.rs` |
| Types | `ceres/src/lfs/lfs_structs.rs` |
