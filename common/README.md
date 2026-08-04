# common

Shared workspace crate: configuration loading, error types, and utilities used by `mono`, `ceres`, `jupiter`, and other crates.

## Configuration

- Default template: [`config/config.toml`](../config/config.toml)
- Local override: [`config/config.local.toml`](../config/config.local.toml) (preferred over `config.toml` when present; gitignored)
- Loader: `common::config::loader::ConfigLoader`
- Env overrides: `MEGA_*` with `__` for nested keys (e.g. `MEGA_LOG__LEVEL`)
- String substitution: `${base_dir}`, `${section.key}` in TOML values

## Errors

- `MegaError` / `MegaResult` — application errors
- `ProtocolError` — Git protocol errors (HTTP mapping in `mono`)
