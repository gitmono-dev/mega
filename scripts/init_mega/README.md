# init_mega

`init_mega.py` is a standalone Mega initialization / sync script. After the Mega service is up, it:

- **Buckal Bundles sync** (default): clones the `toolchains` repo, replaces `buckal-bundles` with the latest from GitHub, commits if there are changes, then uses the Mega API to find and **auto-merge** the corresponding CL (`merge-no-auth`, no human review).

This script is extracted from the server-side initialization logic so you can run initialization manually (locally or in CI) without relying on the server’s internal startup flow. It is also used by the mega-init Job and the periodic buckal sync CronJob.

## Container image

Dockerfile: [`scripts/init_mega/Dockerfile`](Dockerfile) (python3 + git + these scripts).

- CI: `.github/workflows/mega-init-deploy.yml` pushes `mega/mega-init` to ECR Public and Harbor on changes under `scripts/init_mega/`.
- Local: `./scripts/demo/build-demo-images-local.sh mega-init` (optional `--push`).

K8s Job/CronJob command overrides the image entrypoint and runs `python3 scripts/init_mega/init_mega.py --base-url ...`.

## Requirements

- Python 3
- Git (available on the command line)
- A reachable Mega service (HTTP/HTTPS)

## Parameters

- `--base-url BASE_URL`
  - Mega service base URL (default: `https://git.gitmega.com`). Use this to point to a local/dev server, for example: `http://127.0.0.1:8000`
- `--skip-buckal`
  - Skip the Buckal Bundles sync

## Usage examples

Sync buckal-bundles only (default; use default base URL):

```bash
python3 scripts/init_mega/init_mega.py
```

Sync buckal-bundles (override base URL):

```bash
python3 scripts/init_mega/init_mega.py --base-url http://127.0.0.1:8000
```

Show help:

```bash
python3 scripts/init_mega/init_mega.py --help
```

## How it works

### 1) Wait for Mega to be ready

The script polls:

- `GET {base_url}/api/v1/status`

When it returns HTTP 2xx, the service is considered ready and the script proceeds.

### 2) Buckal Bundles sync (default)

In a temporary directory, it performs:

1. Clone `toolchains`:
  - `git clone {base_url}/toolchains.git`
2. Configure the commit identity (repo-local):
  - `git config user.email mega-bot@example.com`
  - `git config user.name Mega Bot`
  - `git config commit.gpgsign false` (and `git commit --no-gpg-sign`) so a global `commit.gpgsign=true` does not fail without a Mega Bot GPG key
3. Remove any existing `toolchains/buckal-bundles` (idempotent re-sync).
4. Clone latest `buckal-bundles`:
  - `git clone --depth 1 https://github.com/buck2hub/buckal-bundles.git`
  - Record upstream short SHA for the commit message
5. Remove `buckal-bundles/.git` so it becomes a regular directory tracked by `toolchains` (vendoring).
6. Commit and push only if there are changes:
  - `git add .`
  - `git commit --no-gpg-sign -m "sync buckal-bundles <sha>"` (skipped if already up to date)
  - `git push`
7. Use Mega APIs to find and auto-merge the corresponding CL:
  - `POST {base_url}/api/v1/cl/list` (paginate open CLs and match `title == "sync buckal-bundles <sha>"`)
  - `POST {base_url}/api/v1/cl/{link}/merge-no-auth`

## Notes

- The script runs `git push`, so your machine must have push access/authentication to the Git service behind `{base_url}`.
- Mega monorepo pushes create a CL; this script auto-merges it via `merge-no-auth` so no human review is required.
- CL discovery depends on an exact title match: `sync buckal-bundles <sha>`.
- Re-running with unchanged upstream content is a no-op (no commit/push/CL).
- This script does not start the Mega service. Start the service first, then run the script.
