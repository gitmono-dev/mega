# Crates.io crates import (third-party/rust/crates)

> **中文上手（架构 / 本地脚本 / Terraform Job 调试）：** [上手指南.md](./上手指南.md)

## Overview

This script imports crates from `crates.io` into Mega as **path-based git repositories** under:

- `third-party/rust/crates/<crates.io-index-path>/<version>`

It reads a local `crates.io-index` checkout, downloads `.crate` tarballs, extracts sources, creates a git repo per crate version, and pushes to Mega via `git-receive-pack` (HTTP).

## Features

- **Fast small-scale mode**: `--crate <name>` reads only selected index files (no full index walk)
- **Sampling mode**: `--limit-crates N` uses a cached crate-name list to avoid full content scans
- **Full import (no `--crate` / `--limit-crates`)**: walks the index **streamingly**—each crate index file is read, then its versions are queued for download/commit/push without building a giant in-memory map first
- Downloads missing `.crate` files and extracts sources
- Creates a git repo per crate version directory and pushes to Mega
- Bearer-token auth (`Authorization: Bearer ...`) for `git push`
- Optional concurrency via `--jobs` (default: serial)
- Optional overwrite behavior via `--force-with-lease` / `--force`

## Requirements

- Python 3
- Git (installed and accessible from command line)

##  Installation

1. Clone this repository or download the script.
2. Ensure `git` is available on your PATH.

## Usage

Minimal arguments:

- `--index`: path to a local [crates.io index](https://github.com/rust-lang/crates.io-index) checkout
- `--crates-dir`: cache directory for downloaded `.crate` files
- `--workdir`: working directory where per-version repos are created
- `--git-base-url`: Mega base URL (e.g. `https://git.gitmega.com` or `http://localhost:8000`)
- `--token`: bearer token used for `git push` (or `MEGA_TOKEN`)

Example:

```
export MEGA_TOKEN="..."
python3 scripts/crates-sync/crates-sync.py \
  --index ~/crates.io-index \
  --crates-dir /tmp/crates-cache \
  --workdir /tmp/mega-crates-work \
  --git-base-url http://localhost:8000 \
  --token "$MEGA_TOKEN"
```

## How It Works

1. The script reads the crates.io index: for a **full** run it streams one crate file at a time; with `--crate` or `--limit-crates` it only reads the selected crates’ index files.
2. For each crate version:
  - It checks if the crate file exists locally, downloading it if necessary.
  - It extracts the crate contents to a version-specific directory.
  - It creates a Git repository for the crate version, commits the contents, and pushes to Mega.
3. The script handles various edge cases, such as invalid versions and empty crate files.
4. Upon completion, it reports the total number of crates processed.

## Configuration

The script expects a `config.json` file in the crates.io index directory, which should contain a `dl` key with the base URL for downloading crates.

## Error Handling

- The script provides warnings for invalid versions, empty crate files, and failed Git operations.
- It skips processing of crates that fail to extract or have invalid versions.

## Customization

You can modify the script to change:

- The remote Git repository URL format (now customizable via the `<git_base_url>` parameter)
- The commit message format
- The logging verbosity
- The concurrency level (`--jobs`)
- The import manifest location (`--manifest`) and crate-name cache (`--crate-name-cache`)

## Limitations

- The script assumes a specific structure for the crates.io index.
- **Full streaming import** does not random-shuffle crate order (walk order follows the filesystem). Use `--limit-crates` if you want a random sample.
- It requires Git to be installed and configured on the system.
- Large-scale processing may take a significant amount of time and disk space.
- This script does **not** configure Git LFS (crates sources should not require LFS).

## Import manifest

Each run updates an import manifest (JSONL) at:

- Default: `<workdir>/crates-import-manifest.jsonl`
- Override: `--manifest /path/to/manifest.jsonl`

For each `crate@version`, the manifest tracks:

- `crate`, `version`
- `status`: `ok` | `fail` | `skip`
- `remote`: full remote URL on Mega
- `last_import_time`: UTC timestamp

On startup:

- If a crate version already has `status=ok` in the manifest, it is **skipped by default**.
- `--force` / `--force-with-lease` only affect `git push`; they **do not** disable manifest skipping (so you can use them for non-fast-forward mirrors without re-importing every crate).
- To intentionally re-import versions that are already `ok`, use `--reimport-ok`.

Each completed task also **appends** one line to the manifest file immediately, so a crash mid-run still keeps finished `ok`/`fail` rows on disk (the run end rewrites a compact file).

## Contributing

Contributions to improve the script are welcome. Please submit pull requests or open issues for any bugs or feature requests.

## Disclaimer

This script is not officially associated with crates.io or the Rust project. Use it responsibly and in accordance with crates.io's terms of service.

## Testing

### Small-scale test (recommended during development)

Use `--crate` to process only a small allowlist of crates. This mode **does not walk the full index tree**; it reads only the corresponding index files, so it is much faster than a full scan.

Import the latest 1 version of `tokio` and `serde`:

```
python3 scripts/crates-sync/crates-sync.py \
  --index ~/crates.io-index \
  --crates-dir /tmp/crates-cache \
  --workdir /tmp/mega-crates-work \
  --git-base-url http://localhost:8000 \
  --token "$MEGA_TOKEN" \
  --crate tokio --crate serde \
  --max-versions-per-crate 1 \
  --jobs 8 \
  --repush-existing

```

Dry-run (download/extract only, no commit/push):

```
python3 scripts/crates-sync/crates-sync.py \
  --index ~/crates.io-index \
  --crates-dir /tmp/crates-cache \
  --workdir /tmp/mega-crates-work \
  --git-base-url http://localhost:8000 \
  --crate tokio --crate serde \
  --max-versions-per-crate 1 \
  --dry-run
```

### Full / sample test (scans the whole index)

If you omit `--crate`, the script will scan the entire `crates.io-index` checkout. For a smaller validation run, use `--limit-crates` to sample a subset, and `--max-versions-per-crate` to cap versions per crate.

Sample 20 crates, latest 1 version each:

```
python3 scripts/crates-sync/crates-sync.py \
  --index ~/crates.io-index \
  --crates-dir /tmp/crates-cache \
  --workdir /tmp/mega-crates-work \
  --git-base-url http://localhost:8000 \
  --token "$MEGA_TOKEN" \
  --limit-crates 20 \
  --jobs 8 \
  --max-versions-per-crate 1
```

### Production-style run (full import)

For a production/import run against a Mega instance (for example `https://git.buck2hub.com`), you typically want:

- A long-lived `crates-dir` on a large disk
- A scratch `workdir` (per-run or per-batch)
- A bearer token with permissions to push into `third-party/rust/crates/**`
- Concurrency enabled via `--jobs`

Example:

```
export MEGA_TOKEN="your-prod-token"

python3 scripts/crates-sync/crates-sync.py \
  --index ~/crates.io-index \
  --crates-dir ~/crates-cache \
  --workdir ~/mega-crates-work \
  --git-base-url https://git.buck2hub.com \
  --token "$MEGA_TOKEN" \
  --jobs 4 \
  --max-versions-per-crate 0 \
  --manifest ~/crates-import-manifest.jsonl
```

Notes:

- The manifest ensures already-imported `crate@version` entries are skipped on subsequent runs.
- Adjust `--jobs` based on your CPU/IO and Mega server capacity.
- Use `--keep-crate-cache` when `--crates-dir` is a shared host cache (e.g. freighter) so successful pushes do not delete `.crate` files.

## Kubernetes Job (onprem)

CI builds `mega/crates-sync` from `scripts/crates-sync/Dockerfile` (workflow: `.github/workflows/crates-sync-deploy.yml`).

The Job entrypoint is `run_job.py`: wait for mono → `bootstrap-init` bot token → optional `git pull` on index → `crates-sync.py` with `--keep-crate-cache` and `--max-versions-per-crate 0` (all versions).

On **mega-rust**, data is expected on the node hostPath:

| Host path | Container mount |
|-----------|-----------------|
| `/opt/data/freighter` | `/freighter` |
| `.../crates.io-index` | `--index` |
| `.../crates` | `--crates-dir` |
| `.../mega-crates-work` | `--workdir` + manifest |

Terraform (`gitmono_stack`):

```hcl
enable_crates_sync              = true
crates_sync_freighter_host_path = "/opt/data/freighter"
crates_sync_node_hostname       = "storage-server-01"  # must match kubectl get nodes
# crates_sync_image = "registry.xuanwu.openatom.cn/mega/crates-sync:<sha>"
```

The Job is scheduled onto that node (`nodeAffinity`), mounts the freighter hostPath, and runs asynchronously (`wait_for_completion = false`). Re-run by bumping `crates_sync_image` (or delete the Job and re-apply).

Manual run on the freighter host (no Job):

```bash
export MEGA_TOKEN="..."
python3 scripts/crates-sync/crates-sync.py \
  --index /opt/data/freighter/crates.io-index \
  --crates-dir /opt/data/freighter/crates \
  --workdir /opt/data/freighter/mega-crates-work \
  --manifest /opt/data/freighter/mega-crates-work/crates-import-manifest.jsonl \
  --git-base-url https://git.rust.xuanwu.openatom.cn \
  --token "$MEGA_TOKEN" \
  --max-versions-per-crate 0 \
  --jobs 2 \
  --keep-crate-cache
```
