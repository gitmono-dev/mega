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
LIBRA_REPO = "https://github.com/web3infra-foundation/libra.git"
COMMIT_MSG = "import buckal-bundles"
IMPORT_SCRIPT_PATH = "import-buck2-deps/import-buck2-deps.py"

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

def api_request(method, url, data=None, headers=None):
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
        with urllib.request.urlopen(req, timeout=10) as response:
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
            resp = api_request("GET", status_url)
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

def run_buckal_bundles_workflow(base_url):
    """Executes the buckal-bundles import workflow."""
    print("--- Starting Buckal Bundles Workflow ---")
    with tempfile.TemporaryDirectory(prefix="mega-init-buckal-") as temp_dir:
        temp_path = Path(temp_dir)
        
        # Clone toolchains
        toolchains_url = f"{base_url.rstrip('/')}/toolchains.git"
        run_git(temp_path, ["clone", toolchains_url])
        
        toolchains_dir = temp_path / "toolchains"
        
        # Config git (repo-local). Disable GPG so CI/dev machines with
        # commit.gpgsign=true globally do not fail without a Mega Bot key.
        run_git(toolchains_dir, ["config", "user.email", GIT_USER_EMAIL])
        run_git(toolchains_dir, ["config", "user.name", GIT_USER_NAME])
        run_git(toolchains_dir, ["config", "commit.gpgsign", "false"])
        
        # Clone buckal-bundles inside
        print("Importing buckal-bundles...")
        run_git(toolchains_dir, ["clone", "--depth", "1", BUCKAL_BUNDLES_REPO])
        
        # Remove .git from buckal-bundles
        buckal_git = toolchains_dir / "buckal-bundles" / ".git"
        if buckal_git.exists():
            if buckal_git.is_dir():
                shutil.rmtree(buckal_git)
            else:
                buckal_git.unlink()
        
        # Commit and push
        run_git(toolchains_dir, ["add", "."])
        status = run_git(toolchains_dir, ["status", "--porcelain"], check=False)
        if not status.stdout.strip():
            print("buckal-bundles already present with no changes; skipping commit/push/merge.")
            return

        run_git(toolchains_dir, ["commit", "--no-gpg-sign", "-m", COMMIT_MSG])
        run_git(toolchains_dir, ["push"])
        
        # Handle merge request
        print("Finding CL to merge...")
        # Give it a few seconds for the CL to be processed
        time.sleep(5)
        
        link = None
        start_find = time.time()
        while time.time() - start_find < 90:
            link = find_cl_link(base_url, COMMIT_MSG)
            if link:
                break
            time.sleep(2)
            
        if not link:
            raise RuntimeError(f"Could not find CL with title '{COMMIT_MSG}'")
            
        print(f"Found CL link: {link}")
        merge_cl(base_url, link)

def run_libra_workflow(base_url, project_root):
    """Executes the libra import workflow."""
    print("--- Starting Libra Workflow ---")
    with tempfile.TemporaryDirectory(prefix="mega-init-libra-") as temp_dir:
        temp_path = Path(temp_dir)
        
        # Clone libra
        print(f"Cloning libra to {temp_path}...")
        run_git(temp_path, ["clone", LIBRA_REPO, "."])
        
        # Resolve script path
        script_path = project_root / IMPORT_SCRIPT_PATH
        if not script_path.exists():
            raise RuntimeError(f"Import script not found at {script_path}")
            
        third_party_path = temp_path / "third-party"
        
        # Run import script
        print("Running import-buck2-deps.py for libra...")
        cmd = [
            "python3", str(script_path),
            "--scan-root", str(third_party_path),
            "--git-base-url", base_url,
            "--no-signoff",
            "--no-gpg-sign",
            "--ui", "plain",
            "--jobs", "6",
            "--retry", "3"
        ]
        
        result = subprocess.run(cmd, cwd=temp_path)
        if result.returncode != 0:
            raise RuntimeError(f"Libra import script failed with exit code {result.returncode}")
        
        print("Libra workflow completed successfully.")

def main():
    parser = argparse.ArgumentParser(description="Mega Server Initialization Script")
    parser.add_argument(
        "--base-url",
        default="https://git.gitmega.com",
        help="Base URL of the Mega server (default: https://git.gitmega.com)",
    )
    parser.add_argument("--skip-buckal", action="store_true", help="Skip buckal bundles workflow")
    parser.add_argument("--skip-libra", action="store_true", help="Skip libra workflow")
    
    args = parser.parse_args()
    
    base_url = args.base_url
        
    print(f"Initializing Mega at {base_url}")
    
    # Resolve project root (where the script is located, one level up from scripts/)
    script_dir = Path(__file__).parent.absolute()
    project_root = script_dir.parent
    
    try:
        # Wait for server
        wait_for_server(base_url)
        
        if not args.skip_buckal:
            run_buckal_bundles_workflow(base_url)
            
        if not args.skip_libra:
            run_libra_workflow(base_url, project_root)
            
        print("\nAll initialization tasks completed successfully!")
        
    except Exception as e:
        print(f"\nInitialization failed: {e}")
        exit(1)

if __name__ == "__main__":
    main()
