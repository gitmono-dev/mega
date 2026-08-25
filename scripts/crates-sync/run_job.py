#!/usr/bin/env python3
"""K8s Job entrypoint for crates-sync.

Waits for mono-engine, bootstraps a bot push token via MEGA_INIT_BOOTSTRAP_SECRET,
then runs crates-sync.py against a freighter hostPath.

Freighter owns updates to crates.io-index and the .crate cache. This entrypoint
treats those trees as read-only (no git pull; no download/delete under crates/).
Optional --pull-index is opt-in only.

Typical freighter hostPath layout (mounted at --freighter-root):

  <root>/crates.io-index   -> --index   (read-only)
  <root>/crates            -> --crates-dir (read-only cache)
  <root>/mega-crates-work  -> --workdir (+ manifest; writable)
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

INIT_BOOTSTRAP_SECRET_ENV = "MEGA_INIT_BOOTSTRAP_SECRET"
INIT_BOOTSTRAP_SECRET_HEADER = "X-Mega-Init-Secret"
INIT_BOOTSTRAP_SECRET_MIN_LEN = 32

SCRIPT_DIR = Path(__file__).resolve().parent
CRATES_SYNC_PY = SCRIPT_DIR / "crates-sync.py"


def api_request(method, url, data=None, headers=None, timeout=10):
    if headers is None:
        headers = {}
    if "accept" not in headers:
        headers["accept"] = "application/json"
    req_data = None
    if data is not None:
        req_data = json.dumps(data).encode("utf-8")
        if "Content-Type" not in headers:
            headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=req_data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            resp_body = response.read().decode("utf-8")
            if 200 <= response.status < 300:
                return json.loads(resp_body) if resp_body else {}
            raise RuntimeError(f"API request failed with status {response.status}: {resp_body}")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"API request to {url} failed: HTTP Error {e.code}: {e.reason}; body={body}") from e
    except Exception as e:
        raise RuntimeError(f"API request to {url} failed: {e}") from e


def wait_for_server(base_url, timeout=300):
    status_url = f"{base_url.rstrip('/')}/api/v1/status"
    start = time.time()
    print(f"Waiting for server at {status_url}...")
    while time.time() - start < timeout:
        try:
            api_request("GET", status_url)
            print("Server is ready.")
            return
        except Exception:
            time.sleep(2)
    raise RuntimeError(f"Server at {base_url} did not become ready within {timeout}s")


def resolve_init_bootstrap_secret(cli_secret=None):
    secret = (cli_secret or "").strip() or os.environ.get(INIT_BOOTSTRAP_SECRET_ENV, "").strip()
    if not secret:
        raise RuntimeError(
            f"bootstrap-init requires {INIT_BOOTSTRAP_SECRET_ENV} "
            "(or --init-secret); must match mono-engine"
        )
    if len(secret) < INIT_BOOTSTRAP_SECRET_MIN_LEN:
        raise RuntimeError(
            f"{INIT_BOOTSTRAP_SECRET_ENV} must be at least "
            f"{INIT_BOOTSTRAP_SECRET_MIN_LEN} characters"
        )
    return secret


def bootstrap_init_bot_token(base_url, init_secret):
    url = f"{base_url.rstrip('/')}/api/v1/bots/bootstrap-init"
    print(f"Bootstrapping init bot token via {url}...")
    resp = api_request(
        "POST",
        url,
        data={},
        headers={INIT_BOOTSTRAP_SECRET_HEADER: init_secret},
        timeout=120,
    )
    if not resp.get("req_result"):
        raise RuntimeError(f"bootstrap-init failed: {resp.get('err_message') or resp}")
    data = resp.get("data") or {}
    token = data.get("token")
    if not token:
        raise RuntimeError(f"bootstrap-init returned no token: {resp}")
    print(f"Got bot token for bot_name={data.get('bot_name')} bot_id={data.get('bot_id')}")
    return token


def maybe_pull_index(index_path: Path) -> None:
    git_dir = index_path / ".git"
    if not git_dir.exists():
        print(f"Index at {index_path} is not a git checkout; skipping pull.")
        return
    print(f"Refreshing crates.io-index at {index_path}...")
    result = subprocess.run(
        ["git", "-C", str(index_path), "pull", "--ff-only"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(
            f"Warning: git pull --ff-only failed (continuing with existing index): "
            f"{(result.stderr or result.stdout or '').strip()}"
        )
    else:
        print((result.stdout or "").strip() or "Index up to date.")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="run_job.py")
    p.add_argument(
        "--base-url",
        default=os.environ.get("MEGA_BASE_URL", "http://mono-engine:8000"),
        help="Mega mono-engine base URL (ClusterIP in-cluster).",
    )
    p.add_argument(
        "--freighter-root",
        default=os.environ.get("CRATES_SYNC_FREIGHTER_ROOT", "/freighter"),
        help="Host freighter mount root (default: /freighter).",
    )
    p.add_argument("--index", default="", help="Override index path (default: <freighter>/crates.io-index).")
    p.add_argument("--crates-dir", default="", help="Override crates cache (default: <freighter>/crates).")
    p.add_argument(
        "--workdir",
        default="",
        help="Override workdir (default: <freighter>/mega-crates-work).",
    )
    p.add_argument("--manifest", default="", help="Override manifest path.")
    p.add_argument("--init-secret", default="", help="Bootstrap secret (or env MEGA_INIT_BOOTSTRAP_SECRET).")
    p.add_argument(
        "--jobs",
        type=int,
        default=2,
        help="Concurrent workers passed to crates-sync (default: 2).",
    )
    p.add_argument(
        "--max-versions-per-crate",
        type=int,
        default=0,
        help="0 = all versions per crate (default for Job).",
    )
    p.add_argument(
        "--pull-index",
        action="store_true",
        help="git pull the crates.io-index checkout before sync (writes to index; off by default).",
    )
    p.add_argument(
        "--no-pull-index",
        action="store_true",
        help="Deprecated no-op: index pull is already off by default (freighter owns index updates).",
    )
    p.add_argument(
        "--wait-timeout",
        type=int,
        default=300,
        help="Seconds to wait for mono /api/v1/status (default: 300).",
    )
    args, extra = p.parse_known_args(argv)

    freighter = Path(args.freighter_root)
    index_path = Path(args.index) if args.index else freighter / "crates.io-index"
    crates_dir = Path(args.crates_dir) if args.crates_dir else freighter / "crates"
    workdir = Path(args.workdir) if args.workdir else freighter / "mega-crates-work"
    manifest = (
        Path(args.manifest)
        if args.manifest
        else workdir / "crates-import-manifest.jsonl"
    )

    if not index_path.is_dir():
        raise SystemExit(f"Index directory not found: {index_path}")
    if not (index_path / "config.json").is_file():
        raise SystemExit(f"Index config.json not found under {index_path}")
    if not crates_dir.is_dir():
        raise SystemExit(
            f"Crates cache directory not found (readonly freighter layout): {crates_dir}"
        )
    workdir.mkdir(parents=True, exist_ok=True)

    base_url = args.base_url.rstrip("/")
    wait_for_server(base_url, timeout=args.wait_timeout)
    init_secret = resolve_init_bootstrap_secret(args.init_secret)
    token = bootstrap_init_bot_token(base_url, init_secret)

    # Freighter owns index/crates updates. Only pull when explicitly requested.
    if args.pull_index:
        if args.no_pull_index:
            print("Warning: --pull-index ignored because --no-pull-index was also set.")
        else:
            maybe_pull_index(index_path)
    elif args.no_pull_index:
        print("Index pull skipped (default; --no-pull-index is redundant).")

    if not CRATES_SYNC_PY.is_file():
        raise SystemExit(f"crates-sync.py not found next to run_job.py: {CRATES_SYNC_PY}")

    cmd = [
        sys.executable,
        "-u",
        str(CRATES_SYNC_PY),
        "--index",
        str(index_path),
        "--crates-dir",
        str(crates_dir),
        "--workdir",
        str(workdir),
        "--manifest",
        str(manifest),
        "--git-base-url",
        base_url,
        "--token",
        token,
        "--max-versions-per-crate",
        str(args.max_versions_per_crate),
        "--jobs",
        str(args.jobs),
        "--keep-crate-cache",
        "--readonly-crate-cache",
        "--status-sticky",
    ]
    cmd.extend(extra)

    printable = [
        sys.executable,
        "-u",
        str(CRATES_SYNC_PY),
        "--index",
        str(index_path),
        "--crates-dir",
        str(crates_dir),
        "--workdir",
        str(workdir),
        "--manifest",
        str(manifest),
        "--git-base-url",
        base_url,
        "--token",
        "***",
        "--max-versions-per-crate",
        str(args.max_versions_per_crate),
        "--jobs",
        str(args.jobs),
        "--keep-crate-cache",
        "--readonly-crate-cache",
        "--status-sticky",
        *extra,
    ]
    print("Running:", " ".join(printable))
    return subprocess.call(cmd)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
