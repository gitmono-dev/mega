# init_mega

`init_mega.py` is a standalone Mega initialization script. After the Mega service is up, it runs a set of “import/initialization” workflows:

- **Buckal Bundles import**: clones the `toolchains` repo, vendors the `buckal-bundles` repo into it and commits the changes, then uses the Mega API to find and merge the corresponding CL.
- **Libra dependency import**: clones the `libra` repo and calls the in-repo script `scripts/import-buck2-deps/import-buck2-deps.py` to import Buck2 dependencies under `third-party/` into Mega.

This script is extracted from the server-side initialization logic so you can run initialization manually (locally or in CI) without relying on the server’s internal startup flow.

## Requirements

- Python 3
- Git (available on the command line)
- A reachable Mega service (HTTP/HTTPS)

## Parameters

- `--base-url BASE_URL`
  - Mega service base URL (default: `https://git.gitmega.com`). Use this to point to a local/dev server, for example: `http://127.0.0.1:8000`
- `--skip-buckal`
  - Skip the Buckal Bundles workflow
- `--skip-libra`
  - Skip the Libra workflow

## Usage examples

Run full initialization (use default base URL):

```bash
python3 scripts/init_mega/init_mega.py
```

Run full initialization (override base URL):

```bash
python3 scripts/init_mega/init_mega.py --base-url http://127.0.0.1:8000
```

Run only Buckal Bundles (skip Libra, override base URL):

```bash
python3 scripts/init_mega/init_mega.py --base-url http://127.0.0.1:8000 --skip-libra
```

Run only Libra (skip Buckal Bundles, override base URL):

```bash
python3 scripts/init_mega/init_mega.py --base-url http://127.0.0.1:8000 --skip-buckal
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

### 2) Buckal Bundles workflow

In a temporary directory, it performs:

1. Clone `toolchains`:
  - `git clone {base_url}/toolchains.git`
2. Configure the commit identity (repo-local):
  - `git config user.email mega-bot@example.com`
  - `git config user.name Mega Bot`
  - `git config commit.gpgsign false` (and `git commit --no-gpg-sign`) so a global `commit.gpgsign=true` does not fail without a Mega Bot GPG key
3. Clone `buckal-bundles` inside the `toolchains` repo:
  - `git clone --depth 1 https://github.com/buck2hub/buckal-bundles.git`
4. Remove `buckal-bundles/.git` so it becomes a regular directory tracked by `toolchains` (vendoring).
5. Commit and push:
  - `git add .`
  - `git commit --no-gpg-sign -m "import buckal-bundles"` (skipped if already imported / no changes)
  - `git push`
6. Use Mega APIs to find and merge the corresponding CL:
  - `POST {base_url}/api/v1/cl/list` (paginate open CLs and match `title == "import buckal-bundles"`)
  - `POST {base_url}/api/v1/cl/{link}/merge-no-auth`

### 3) Libra workflow

Also in a temporary directory:

1. Clone `libra`:
  - `git clone https://github.com/gitmono-dev/libra.git .`
2. Use `third-party/` in the temporary directory as the scan root, and call the existing in-repo import script:
  - `python3 scripts/import-buck2-deps/import-buck2-deps.py --scan-root <temp>/third-party --git-base-url {base_url} ...`

## Notes

- The script runs `git push`, so your machine must have push access/authentication to the Git service behind `{base_url}`.
- CL discovery for Buckal Bundles depends on an exact title match: `import buckal-bundles` (customize via the `COMMIT_MSG` constant in the script if needed).
- This script does not start the Mega service. Start the service first, then run the script.
