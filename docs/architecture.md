# Architecture

Mega is a Rust workspace centered on the `mono` server binary. Domain logic lives in `ceres`; persistence in `jupiter` + `callisto`; schema migrations in `jupiter-migrate`.

## Workspace crates

| Crate | Role |
|-------|------|
| `mono` | HTTP/SSH server binary, REST routers, composition root (`bootstrap::AppContext`) |
| `ceres` | Git transport, application services, HTTP DTOs — see [ceres/README.md](../ceres/README.md) |
| `jupiter` | Storage layer and services over SeaORM |
| `jupiter/callisto` | Generated SeaORM entities |
| `jupiter-migrate` | SeaORM migrations (feature-gated; see [jupiter-migrate/README.md](../jupiter-migrate/README.md)) |
| `api-model` | Wire protocol between `mono` and Orion (buck2, artifacts, pagination) |
| `common` | Shared config, errors, utilities |
| `io-orbit` | Object storage abstraction (local, S3, GCS) |
| `saturn` | Cedar policy engine |
| `vault` | Cryptographic vault (PGP, PKI, secrets) |
| `orion` | Build runner (Buck2 WebSocket client) |
| `orion-server` | Build task server |
| `orion-scheduler` | VM / runner orchestration |
| `clients/orion-client` | HTTP client for Orion APIs |

Frontend UI: `moon/apps/web` (Next.js).

## Runtime assembly

```text
mono CLI
  └─ bootstrap::AppContext (storage, vault, config, redis)
       ├─ HTTP server: REST + Git Smart HTTP + Swagger
       ├─ SSH server: Git over SSH
       └─ TransportRuntime (ceres) — Git pack handlers + application event bus
```

`mono` constructs a `TransportRuntime` with storage, `GitObjectCache`, and `RuntimeApplicationHandler`, then wires it to Git protocol handlers and REST routes. Push flow details: [ceres/README.md](../ceres/README.md#git-push-event-flow).

## Error handling boundaries

REST and application logic use `MegaError` (`common::errors`), mapped to HTTP by `ApiError` in `mono`. Git Smart HTTP/SSH transport uses `ProtocolError` with `protocol_error::into_response`. See [ceres/README.md#error-type-boundaries](../ceres/README.md#error-type-boundaries).

## DTO and module boundaries

HTTP/OpenAPI types live in `ceres/model`. `mono` routers must not import `jupiter::model`, `callisto`, or `jupiter::service` directly — use `ceres::model` and `MonoApiService` facades. `ceres/src/transport` must not depend on `MonoApiService`. CI enforces these rules in [`.github/workflows/base.yml`](../.github/workflows/base.yml). Full rules: [ceres/README.md#model-boundary](../ceres/README.md#model-boundary).

## HTTP API discovery

When the `mono` HTTP server is running (default port `8000`):

| Resource | URL |
|----------|-----|
| Swagger UI | `http://localhost:8000/swagger-ui` |
| OpenAPI JSON | `http://localhost:8000/api/openapi.json` |

REST handlers are defined in `mono/src/api/` with `utoipa` annotations. Do not maintain a separate hand-written API catalog. Frontend typed clients are generated from those specs — do not hand-edit `moon/api/gen/gitmono.json` or `moon/packages/types/generated.ts`; see [moon/api/README.md](../moon/api/README.md).

### Git LFS

LFS handlers are mounted at two equivalent prefixes:

| Audience | Base path |
|----------|-----------|
| Git LFS clients | `<repo>.git/info/lfs/...` (e.g. `/project/foo.git/info/lfs/objects/batch`) |
| OpenAPI / tools | `/api/v1/lfs/...` |

See [lfs-api.md](lfs-api.md).

### Git Smart HTTP / SSH

Standard `info/refs`, `git-upload-pack`, and `git-receive-pack` under repo-scoped paths such as `/project/...` and `/third-party/...`.

## Database schema

Source of truth:

- Migrations: `jupiter-migrate/src/migration/`
- Entities: `jupiter/callisto/src/`

Migrations apply automatically when `mono` starts (`jupiter` `migrate` feature enabled). No hand-maintained ER diagram in docs.

## Orion ecosystem

- Runner: [orion/README.md](../orion/README.md)
- Deployment: [orion/docs/deployment.md](../orion/docs/deployment.md)
- Object access from Orion: [orion-mega-object-access.md](orion-mega-object-access.md) (draft)

## Related projects (external)

- [Libra](https://github.com/web3infra-foundation/libra) — Git-compatible agent client
- [ScorpioFS](https://github.com/web3infra-foundation/scorpiofs) — FUSE filesystem for monorepo folders
