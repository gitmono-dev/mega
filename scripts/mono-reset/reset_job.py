#!/usr/bin/env python3
"""K8s Job entrypoint: wipe mono git/CL data (keep login), clear RustFS git/lfs, re-init.

Order:
  1. Scale mono-engine (+ optional orion-server) to 0
  2. TRUNCATE public PG tables except keep-list (MySQL untouched)
  3. mc rm bucket prefixes git/ and lfs/
  4. Scale deployments back
  5. Wait for mono /api/v1/status (boot runs init_monorepo)
  6. Run scripts/init_mega/init_mega.py (buckal-bundles sync) unless --skip-buckal
"""

from __future__ import annotations

import argparse
import json
import os
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from wipe_sql import wipe_public_tables

SCRIPT_DIR = Path(__file__).resolve().parent
INIT_MEGA_PY = SCRIPT_DIR.parent / "init_mega" / "init_mega.py"

SA_TOKEN_PATH = Path("/var/run/secrets/kubernetes.io/serviceaccount/token")
SA_CA_PATH = Path("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt")
SA_NS_PATH = Path("/var/run/secrets/kubernetes.io/serviceaccount/namespace")


def env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def api_request(method: str, url: str, data=None, headers=None, timeout: int = 15):
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
            body = response.read().decode("utf-8")
            if 200 <= response.status < 300:
                return json.loads(body) if body else {}
            raise RuntimeError(f"API {method} {url} -> {response.status}: {body}")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"API {method} {url} failed: HTTP {e.code}: {body}") from e


def wait_for_server(base_url: str, timeout: int = 300) -> None:
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


def k8s_session():
    host = env("KUBERNETES_SERVICE_HOST")
    port = env("KUBERNETES_SERVICE_PORT", "443")
    if not host or not SA_TOKEN_PATH.is_file():
        raise RuntimeError(
            "In-cluster Kubernetes credentials not found; "
            "Job must run with a ServiceAccount (or pass --skip-scale)."
        )
    token = SA_TOKEN_PATH.read_text().strip()
    ns = env("MONO_NAMESPACE") or (
        SA_NS_PATH.read_text().strip() if SA_NS_PATH.is_file() else ""
    )
    if not ns:
        raise RuntimeError("MONO_NAMESPACE unset and no serviceaccount namespace")
    ctx = ssl.create_default_context(cafile=str(SA_CA_PATH)) if SA_CA_PATH.is_file() else None
    return f"https://{host}:{port}", token, ns, ctx


def k8s_request(method: str, path: str, token: str, base: str, ctx, data=None):
    url = f"{base}{path}"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
    }
    body = None
    if data is not None:
        body = json.dumps(data).encode("utf-8")
        headers["Content-Type"] = "application/strategic-merge-patch+json"
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, context=ctx, timeout=30) as resp:
            raw = resp.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        err = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"k8s {method} {path} -> HTTP {e.code}: {err}") from e


def get_replicas(base: str, token: str, ns: str, ctx, name: str) -> int:
    path = f"/apis/apps/v1/namespaces/{ns}/deployments/{name}/scale"
    scale = k8s_request("GET", path, token, base, ctx)
    return int(scale.get("spec", {}).get("replicas", 0))


def set_replicas(base: str, token: str, ns: str, ctx, name: str, replicas: int) -> None:
    path = f"/apis/apps/v1/namespaces/{ns}/deployments/{name}/scale"
    # Scale subresource accepts merge patch on spec.replicas
    url = f"{base}{path}"
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
        "Content-Type": "application/merge-patch+json",
    }
    body = json.dumps({"spec": {"replicas": replicas}}).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers=headers, method="PATCH")
    try:
        with urllib.request.urlopen(req, context=ctx, timeout=30) as resp:
            resp.read()
    except urllib.error.HTTPError as e:
        err = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"scale {name} -> {replicas} failed: HTTP {e.code}: {err}") from e
    print(f"Scaled deployment/{name} to {replicas}")


def wait_replicas(base: str, token: str, ns: str, ctx, name: str, want: int, timeout: int = 300):
    path = f"/apis/apps/v1/namespaces/{ns}/deployments/{name}"
    start = time.time()
    while time.time() - start < timeout:
        dep = k8s_request("GET", path, token, base, ctx)
        status = dep.get("status", {})
        ready = int(status.get("readyReplicas") or 0)
        replicas = int(status.get("replicas") or 0)
        if want == 0 and replicas == 0:
            print(f"deployment/{name} scaled to 0")
            return
        if want > 0 and ready >= want:
            print(f"deployment/{name} readyReplicas={ready}")
            return
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for deployment/{name} replicas={want}")


def scale_deployments(
    names: list[str],
    *,
    to_zero: bool,
    saved: dict[str, int] | None,
    dry_run: bool,
) -> dict[str, int]:
    base, token, ns, ctx = k8s_session()
    result: dict[str, int] = dict(saved or {})
    for name in names:
        if not name:
            continue
        if to_zero:
            current = get_replicas(base, token, ns, ctx, name)
            result[name] = current if current > 0 else result.get(name, 1)
            print(f"deployment/{name} current replicas={current}")
            if dry_run:
                print(f"DRY-RUN: would scale {name} -> 0")
                continue
            set_replicas(base, token, ns, ctx, name, 0)
            wait_replicas(base, token, ns, ctx, name, 0)
        else:
            want = result.get(name, 1)
            if want < 1:
                want = 1
            if dry_run:
                print(f"DRY-RUN: would scale {name} -> {want}")
                continue
            set_replicas(base, token, ns, ctx, name, want)
            wait_replicas(base, token, ns, ctx, name, want)
    return result


def resolve_db_url(cli: str | None) -> str:
    db_url = (
        (cli or "").strip()
        or env("MEGA_DATABASE__DB_URL")
        or env("DATABASE_URL")
    )
    if not db_url:
        raise RuntimeError("Missing --db-url / MEGA_DATABASE__DB_URL / DATABASE_URL")
    return db_url


def wipe_s3(*, dry_run: bool) -> None:
    endpoint = env("MEGA_OBJECT_STORAGE__S3__ENDPOINT_URL") or env("S3_ENDPOINT")
    access = env("MEGA_OBJECT_STORAGE__S3__ACCESS_KEY_ID") or env("S3_ACCESS_KEY")
    secret = env("MEGA_OBJECT_STORAGE__S3__SECRET_ACCESS_KEY") or env("S3_SECRET_KEY")
    bucket = env("MEGA_OBJECT_STORAGE__S3__BUCKET") or env("S3_BUCKET")
    if not all([endpoint, access, secret, bucket]):
        raise RuntimeError(
            "S3 wipe requires MEGA_OBJECT_STORAGE__S3__ENDPOINT_URL, "
            "ACCESS_KEY_ID, SECRET_ACCESS_KEY, BUCKET (or S3_* aliases)"
        )

    def run(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess:
        print(f"Running: {' '.join(cmd)}")
        return subprocess.run(cmd, capture_output=True, text=True, check=check)

    run(["mc", "alias", "set", "rfs", endpoint, access, secret])
    print("=== S3 before ===")
    before = run(["mc", "du", f"rfs/{bucket}"], check=False)
    print(before.stdout or before.stderr)
    for prefix in ("git", "lfs"):
        target = f"rfs/{bucket}/{prefix}/"
        if dry_run:
            print(f"DRY-RUN: would mc rm --recursive --force --dangerous {target}")
            continue
        print(f"Wiping {target} ...")
        # --dangerous required for non-empty recursive remove of prefix
        rm = run(
            ["mc", "rm", "--recursive", "--force", "--dangerous", target],
            check=False,
        )
        if rm.stdout:
            print(rm.stdout)
        if rm.stderr:
            print(rm.stderr)
        if rm.returncode not in (0,):
            # Empty prefix may still exit 0; treat non-zero as warning if "does not exist"
            err = (rm.stderr or "") + (rm.stdout or "")
            if "does not exist" in err.lower() or "not found" in err.lower():
                print(f"Prefix {prefix}/ absent; ok")
            else:
                print(f"WARN: mc rm {prefix}/ exited {rm.returncode}")
    print("=== S3 after ===")
    after = run(["mc", "du", f"rfs/{bucket}"], check=False)
    print(after.stdout or after.stderr)
    print("S3_WIPE_OK")


def run_init_mega(base_url: str, *, skip_buckal: bool, init_secret: str | None) -> None:
    if not INIT_MEGA_PY.is_file():
        raise RuntimeError(f"init_mega.py not found at {INIT_MEGA_PY}")
    cmd = [sys.executable, "-u", str(INIT_MEGA_PY), "--base-url", base_url]
    if skip_buckal:
        cmd.append("--skip-buckal")
    if init_secret:
        cmd.extend(["--init-secret", init_secret])
    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"init_mega.py exited {result.returncode}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base-url",
        default=env("MONO_BASE_URL") or "http://mono-engine:8000",
        help="In-cluster mono-engine base URL",
    )
    parser.add_argument("--db-url", default=None)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--skip-s3", action="store_true")
    parser.add_argument("--skip-buckal", action="store_true")
    parser.add_argument("--skip-scale", action="store_true")
    parser.add_argument(
        "--init-secret",
        default=None,
        help="Passed through to init_mega.py (default: MEGA_INIT_BOOTSTRAP_SECRET)",
    )
    args = parser.parse_args(argv)

    mono_deploy = env("MONO_DEPLOYMENT", "mono-engine")
    orion_deploy = env("ORION_DEPLOYMENT", "orion-server")
    scale_names = [n for n in (mono_deploy, orion_deploy) if n]

    print("=== mono-reset start ===")
    print(f"base_url={args.base_url} dry_run={args.dry_run}")

    saved_replicas: dict[str, int] = {}
    try:
        if not args.skip_scale:
            print("--- scale down writers ---")
            saved_replicas = scale_deployments(
                scale_names, to_zero=True, saved=None, dry_run=args.dry_run
            )
        else:
            print("Skipping scale (--skip-scale)")

        print("--- wipe Postgres (keep login tables) ---")
        db_url = resolve_db_url(args.db_url)
        wipe_public_tables(db_url, dry_run=args.dry_run)

        if args.skip_s3:
            print("Skipping S3 wipe (--skip-s3)")
        else:
            print("--- wipe RustFS git/ + lfs/ ---")
            wipe_s3(dry_run=args.dry_run)

        if not args.skip_scale:
            print("--- scale writers back ---")
            scale_deployments(
                scale_names, to_zero=False, saved=saved_replicas, dry_run=args.dry_run
            )

        if args.dry_run:
            print("DRY-RUN: skip wait / init_mega")
            print("=== mono-reset dry-run done ===")
            return 0

        print("--- wait for mono + re-init ---")
        wait_for_server(args.base_url, timeout=300)
        # Give init_monorepo a moment after status becomes ready
        time.sleep(3)
        run_init_mega(
            args.base_url,
            skip_buckal=args.skip_buckal,
            init_secret=args.init_secret,
        )
        print("=== mono-reset complete ===")
        return 0
    except Exception as e:
        print(f"\nmono-reset FAILED: {e}", file=sys.stderr)
        # Best-effort restore writers so the cluster is not left at 0 replicas
        if not args.skip_scale and saved_replicas and not args.dry_run:
            try:
                print("Attempting to restore deployment replicas after failure...")
                scale_deployments(
                    scale_names, to_zero=False, saved=saved_replicas, dry_run=False
                )
            except Exception as restore_err:
                print(f"Restore scale failed: {restore_err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
