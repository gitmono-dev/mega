# mono

Mega monorepo server binary: HTTP REST + Git Smart HTTP, SSH Git, and service composition.

## Entry points

- Binary: `src/main.rs` → `mono::cli::parse`
- Library: `src/lib.rs` (CLI, bootstrap, API routers, Git protocol)

## CLI

```bash
cargo run --bin mono -- service http
cargo run --bin mono -- service ssh
cargo run --bin mono -- service multi http ssh
```

Config: `--config path/to/config.toml` or `MEGA_CONFIG` env var.

When neither is set, mono loads from the current working directory: `config/config.local.toml` if it exists (gitignored local override), otherwise `config/config.toml`. See [config/config.local.toml.example](../config/config.local.toml.example).

## Layout

| Path | Role |
|------|------|
| `src/bootstrap/` | `AppContext` — storage, vault, config, redis |
| `src/api/` | REST routers, OpenAPI (`utoipa`), Swagger |
| `src/git_protocol/` | Smart HTTP / SSH Git |
| `src/server/` | HTTP and SSH server startup |
| `src/commands/` | CLI subcommands |

Domain logic is in `ceres`; see [ceres/README.md](../ceres/README.md).
