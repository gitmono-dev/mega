#!/usr/bin/env python3

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

# Constants
GIT_USER_EMAIL = "mega-bot@example.com"
GIT_USER_NAME = "Mega Bot"
BUCKAL_BUNDLES_REPO = "https://github.com/buck2hub/buckal-bundles.git"
INIT_BOOTSTRAP_SECRET_ENV = "MEGA_INIT_BOOTSTRAP_SECRET"
INIT_BOOTSTRAP_SECRET_HEADER = "X-Mega-Init-Secret"
INIT_BOOTSTRAP_SECRET_MIN_LEN = 32

def run_git(cwd, args, check=True):
    """Executes a git command in the specified directory."""
    cmd = ["git"] + list(args)
    print(f"Running: {' '.join(cmd)} in {cwd}")
    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if check and result.returncode != 0:
        print(f"Error: Git command failed with exit code {result.returncode}")
        print(f"Stdout: {result.stdout}")
        print(f"Stderr: {result.stderr}")
        raise RuntimeError(f"Git command failed: {' '.join(cmd)}")
    return result

def api_request(method, url, data=None, headers=None, timeout=10):
    """Performs an HTTP API request."""
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
            if response.status >= 200 and response.status < 300:
                return json.loads(resp_body) if resp_body else {}
            else:
                raise RuntimeError(f"API request failed with status {response.status}: {resp_body}")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"API request to {url} failed: HTTP Error {e.code}: {e.reason}; body={body}") from e
    except Exception as e:
        raise RuntimeError(f"API request to {url} failed: {e}") from e

def wait_for_server(base_url, timeout=60):
    """Waits for the Mega server to be ready."""
    status_url = f"{base_url.rstrip('/')}/api/v1/status"
    start_time = time.time()
    print(f"Waiting for server at {status_url}...")
    
    while time.time() - start_time < timeout:
        try:
            api_request("GET", status_url)
            # In Rust code, it checks if status is success.
            # Here we assume if api_request doesn't raise, it's success.
            print("Server is ready.")
            return True
        except Exception:
            time.sleep(2)
            
    raise RuntimeError(f"Server at {base_url} did not become ready within {timeout}s")

def find_cl_link(base_url, title, max_pages=5):
    """Finds the CL link for a given title."""
    list_url = f"{base_url.rstrip('/')}/api/v1/cl/list"
    
    for page in range(1, max_pages + 1):
        body = {
            "pagination": {
                "page": page,
                "per_page": 20
            },
            "additional": {
                "sort_by": "created_at",
                "status": "open",
                "asc": False
            }
        }
        
        try:
            resp = api_request("POST", list_url, data=body)
            if not resp.get("req_result"):
                print(f"Warning: CL list request failed: {resp.get('err_message')}")
                continue
            
            items = resp.get("data", {}).get("items", [])
            for cl in items:
                if cl.get("title") == title:
                    return cl.get("link")
        except Exception as e:
            print(f"Warning: Failed to fetch CL list page {page}: {e}")
            
    return None

def merge_cl(base_url, link, timeout=60):
    """Merges a CL by its link."""
    merge_url = f"{base_url.rstrip('/')}/api/v1/cl/{link}/merge-no-auth"
    start_time = time.time()
    
    print(f"Attempting to merge CL: {link}")
    while time.time() - start_time < timeout:
        try:
            resp = api_request("POST", merge_url)
            if resp.get("req_result"):
                print(f"Successfully merged CL: {link}")
                return True
            else:
                print(f"Merge pending: {resp.get('err_message')}")
        except Exception as e:
            # urllib HTTPError body often has the real reason (e.g. CL ref not found).
            detail = ""
            if hasattr(e, "__cause__") and e.__cause__ is not None:
                cause = e.__cause__
                if hasattr(cause, "read"):
                    try:
                        detail = cause.read().decode("utf-8", errors="replace")
                    except Exception:
                        detail = str(cause)
                else:
                    detail = str(cause)
            print(f"Merge attempt failed: {e}" + (f" body={detail}" if detail else ""))
        
        time.sleep(2)
        
    raise RuntimeError(f"Failed to merge CL {link} within {timeout}s")

def resolve_init_bootstrap_secret(cli_secret=None):
    """Resolve shared secret for bootstrap-init (CLI flag or env)."""
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
    """Obtain a fresh mega-init bot push token (shared-secret gated)."""
    url = f"{base_url.rstrip('/')}/api/v1/bots/bootstrap-init"
    print(f"Bootstrapping init bot token via {url}...")
    # RSA keygen for a new bot can take a few seconds.
    resp = api_request(
        "POST",
        url,
        data={},
        headers={INIT_BOOTSTRAP_SECRET_HEADER: init_secret},
        timeout=120,
    )
    if not resp.get("req_result"):
        raise RuntimeError(
            f"bootstrap-init failed: {resp.get('err_message') or resp}"
        )
    data = resp.get("data") or {}
    token = data.get("token")
    if not token:
        raise RuntimeError(f"bootstrap-init returned no token: {resp}")
    print(
        f"Got bot token for bot_name={data.get('bot_name')} bot_id={data.get('bot_id')}"
    )
    return token


def run_buckal_bundles_workflow(base_url, init_secret):
    """Syncs latest buckal-bundles into toolchains (idempotent replace + auto-merge)."""
    print("--- Starting Buckal Bundles Sync ---")
    bot_token = bootstrap_init_bot_token(base_url, init_secret)
    auth_header = f"Authorization: Bearer {bot_token}"

    with tempfile.TemporaryDirectory(prefix="mega-init-buckal-") as temp_dir:
        temp_path = Path(temp_dir)
        
        # Clone toolchains
        toolchains_url = f"{base_url.rstrip('/')}/toolchains.git"
        run_git(temp_path, ["clone", toolchains_url])
        
        toolchains_dir = temp_path / "toolchains"
        buckal_dir = toolchains_dir / "buckal-bundles"
        
        # Config git (repo-local). Disable GPG so CI/dev machines with
        # commit.gpgsign=true globally do not fail without a Mega Bot key.
        run_git(toolchains_dir, ["config", "user.email", GIT_USER_EMAIL])
        run_git(toolchains_dir, ["config", "user.name", GIT_USER_NAME])
        run_git(toolchains_dir, ["config", "commit.gpgsign", "false"])
        
        # Replace any existing vendored copy so re-runs stay idempotent.
        if buckal_dir.exists():
            print("Removing existing toolchains/buckal-bundles before sync...")
            shutil.rmtree(buckal_dir)
        
        print("Cloning latest buckal-bundles...")
        run_git(toolchains_dir, ["clone", "--depth", "1", BUCKAL_BUNDLES_REPO])
        
        # Capture upstream short SHA before stripping .git
        sha_result = run_git(buckal_dir, ["rev-parse", "--short", "HEAD"])
        short_sha = sha_result.stdout.strip()
        commit_msg = f"bot: sync buckal-bundles {short_sha}"
        print(f"Upstream buckal-bundles at {short_sha}")
        
        # Remove .git from buckal-bundles so it becomes a regular directory
        buckal_git = buckal_dir / ".git"
        if buckal_git.exists():
            if buckal_git.is_dir():
                shutil.rmtree(buckal_git)
            else:
                buckal_git.unlink()
        
        # Commit and push
        run_git(toolchains_dir, ["add", "."])
        status = run_git(toolchains_dir, ["status", "--porcelain"], check=False)
        if not status.stdout.strip():
            print("buckal-bundles already up to date; skipping commit/push/merge.")
            return

        run_git(toolchains_dir, ["commit", "--no-gpg-sign", "-m", commit_msg])
        # Authenticated push with mega-init bot token (git-receive-pack requires auth).
        run_git(
            toolchains_dir,
            ["-c", f"http.extraHeader={auth_header}", "push"],
        )
        
        # Handle merge request (auto-merge, no human review)
        print("Finding CL to merge...")
        # Give it a few seconds for the CL to be processed
        time.sleep(5)
        
        link = None
        start_find = time.time()
        while time.time() - start_find < 90:
            link = find_cl_link(base_url, commit_msg)
            if link:
                break
            time.sleep(2)
            
        if not link:
            raise RuntimeError(f"Could not find CL with title '{commit_msg}'")
            
        print(f"Found CL link: {link}")
        merge_cl(base_url, link)

def main():
    parser = argparse.ArgumentParser(
        description="Mega initialization / buckal-bundles sync script"
    )
    parser.add_argument(
        "--base-url",
        default="https://git.gitmega.com",
        help="Base URL of the Mega server (default: https://git.gitmega.com)",
    )
    parser.add_argument(
        "--skip-buckal",
        action="store_true",
        help="Skip buckal-bundles sync",
    )
    parser.add_argument(
        "--init-secret",
        default=None,
        help=(
            f"Shared secret for POST /api/v1/bots/bootstrap-init "
            f"(default: env {INIT_BOOTSTRAP_SECRET_ENV})"
        ),
    )
    
    args = parser.parse_args()
    
    base_url = args.base_url
        
    print(f"Initializing Mega at {base_url}")
    
    try:
        # Wait for server
        wait_for_server(base_url)
        
        if not args.skip_buckal:
            init_secret = resolve_init_bootstrap_secret(args.init_secret)
            run_buckal_bundles_workflow(base_url, init_secret)
            
        print("\nAll initialization tasks completed successfully!")
        
    except Exception as e:
        print(f"\nInitialization failed: {e}")
        exit(1)

if __name__ == "__main__":
    main()
