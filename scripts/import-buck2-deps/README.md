# import-buck2-deps

## Overview

This script imports a local Buck2 third-party Rust dependency tree (defaults to scanning `<repo-root>/third-party`) into Mega at the granularity of “crate version directories”. Each version directory is initialized as its own Git repository and pushed to `<git-base-url>/<rel-path>`.

The Mega relative path is derived starting from the first `third-party/` path component. This means that for the same `third-party/...` directory layout, you get consistent remote paths no matter where you scan from.

## Features

- Automatically discovers import targets by scanning the directory tree and identifying version directories containing a `BUCK` file (by default, version directories must look like `x.y.z`)
- Skips a version directory only when an **ancestor** directory (still inside the scan root) is itself a Git repository, to avoid duplicate imports
- Self-heals interrupted runs: a leftover `.git` sitting directly on a version directory (from a run killed before cleanup) is treated as stale, removed, and re-imported instead of being silently skipped forever (counted as `recovered_stale_git` in the discovery summary; under `--dry-run` it is reported but not deleted)
- For each import target, automatically runs: repo init, branch create/switch, initial commit, remote configuration, and push to Mega
- Always removes the per-repo `.git` directory after processing so reruns rediscover candidates; a final sweep also runs when the whole operation ends — including on `Ctrl-C`/errors — so no generated `.git` survives the run
- Optionally rewrites `//third-party/...` dependency labels in `BUCK` to `//...` (for buckal generated artifacts)
- Supports concurrent imports (`--jobs`)
- Supports an interactive UI (`--ui`): rich (if available) or plain; in rich mode, logs keep only the most recent 12 lines and show a Results summary at the end
- Supports fail-fast: stop after the first failure (skipped tasks are not counted as Failed)

## Dependencies

- Python 3
- Git (available on the command line)
- Optional: the `rich` Python library (for the rich UI under `--ui rich` or `--ui auto`)

## Parameters

- `--scan-root PATH`: Scan root (default: `<repo-root>/third-party`)
- `--git-base-url https://git.gitmega.com`: Git base URL; remote is `<git-base-url>/<rel-path>`
- `--remote-name mega`: Remote name
- `--branch main`: Branch name to initialize/push
- `--include-non-semver`: Allow version directory names that are not in `x.y.z` format
- `--buckal-generated`: Rewrite `//third-party/...` labels in `BUCK` to `//...`
- `--no-signoff`: Do not add `-s` to commits
- `--no-gpg-sign`: Do not add `-S` to commits
- `--force`: Use force push
- `--dry-run`: Print planned actions without executing git operations
- `--jobs N`: Number of repos to process concurrently (default: 1)
- `--ui auto|rich|plain`: Output mode; `auto` enables rich when available
- `--fail-fast`: Stop after the first failure
- `--limit N`: Only process the first N import targets (for small-scope validation)
- `--retry N`: Retry failed repo imports up to N times (default: 0)

## Usage Examples

Example 1: Run the import (defaults):

```bash
python3 scripts/import-buck2-deps/import-buck2-deps.py
```

Example 2: Specify the scan root (absolute path):

```bash
python3 scripts/import-buck2-deps/import-buck2-deps.py \
  --scan-root /path/to/third-party/
```

Example 3: Specify the scan root and import concurrently:

```bash
python3 scripts/import-buck2-deps/import-buck2-deps.py \
  --scan-root /Users/Yetianxing/workspace/rk8s/third-party \
  --jobs 8 \
  --git-base-url https://git.rk8s.xuanwu.openatom.cn
```

Example 4: Preview planned imports (no git operations):

```bash
python3 scripts/import-buck2-deps/import-buck2-deps.py --dry-run
```

## Workflow

1. Scan all directories under `--scan-root` and find directories containing `BUCK` as candidate version directories
2. Filter out candidates that already have a Git repository somewhere above them
3. For each candidate version directory (for example, `tokio/1.48.0`), perform:
   - Initialize or switch to the target branch
   - (Optional) Rewrite `BUCK` dependency labels (`--buckal-generated`)
   - If the repo has no commits yet, create an initial commit (by default includes `-s -S`)
   - Configure/update the remote and push to `<git-base-url>/<rel-path>`
   - Remove the local `.git` directory under the version directory
4. If `--retry` is set, retry only the failed repos for up to N additional attempts
5. Under the rich UI, show a Results summary at the end: Succeeded/Failed/Total, and list failed repos with reasons

## Configuration

- Remote URL: `--git-base-url` (default: `https://git.gitmega.com`)
- Initial commit signing:
  - Defaults to `-s` (Signed-off-by) and `-S` (GPG signing)
  - Disable with `--no-signoff` / `--no-gpg-sign`

## Error Handling

- If the scan root does not exist, is not a directory, or is not readable: print an Error and exit with code 2
- If a child directory cannot be accessed during scanning: print a Warning and continue scanning other directories
- If a single repo import fails: list the failed repo and reason in the rich UI Results
- After each repo attempt (success or failure), the script removes the local `.git` directory under that repo directory so retries/reruns start cleanly
- With `--fail-fast`: stop after the first failure; skipped tasks are not counted as Failed
- With `--retry N`: retry failed repos up to N times; exit non-zero if still failing after retries

## Customization

- Adjust scan scope: `--scan-root`
- Adjust concurrency: `--jobs`
- Adjust UI: `--ui`
- Adjust push behavior: `--force`, `--branch`, `--remote-name`, `--git-base-url`
- Adjust version directory detection: `--include-non-semver`

## Limitations

- If `--scan-root` is omitted, must be run inside a git checkout (the script locates the repo root via `git rev-parse --show-toplevel`)
- The directory structure must be under the `third-party/` tree to derive stable Mega paths
- Pushing requires local git access and authentication for the remote

## Contributing

PRs and issues are welcome to improve scanning rules, UI output, or error handling.

## Disclaimer

This script is an internal repository tool for automating the import workflow. Use it only when you have the appropriate permissions and have verified the target paths.
