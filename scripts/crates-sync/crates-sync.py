#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import queue
import random
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path
import threading
from collections import deque
from typing import Dict, Tuple

# ANSI color codes
GREEN = '\033[92m'
BLUE = '\033[94m'
RED = '\033[91m'
RESET = '\033[0m'

VERBOSE = False
DOWNLOAD_PROGRESS = False
DOWNLOAD_PROGRESS_INTERVAL_S = 1.0

_print_lock = threading.Lock()
_status_block_active = False
_status_block_last_lines: list[str] = []
_status_block_last_lens: list[int] = []

_download_state_lock = threading.Lock()
_active_downloads: set[str] = set()

_stage_state_lock = threading.Lock()
_active_extracts: set[str] = set()
_active_pushes: set[str] = set()
_waiting_push: set[str] = set()

STATUS_HEARTBEAT = False
STATUS_HEARTBEAT_INTERVAL_S = 15.0
STATUS_STICKY = False

_push_ok_lock = threading.Lock()
_push_ok_times: deque[float] = deque()
_push_fail_times: deque[float] = deque()
_push_ok_total = 0
_push_fail_total = 0
_run_start_mono: float | None = None

# Overall import progress (shared with heartbeat).
_progress_lock = threading.Lock()
_progress_index_crates = 0
_progress_versions_queued = 0
_progress_versions_done = 0
_progress_ok = 0
_progress_skip = 0
_progress_fail = 0
_progress_scan_complete = False
_progress_total: int | None = None  # known after scan finishes (or non-streaming start)
_progress_jobs = 1
_work_queue: queue.Queue | None = None

def _record_push_ok() -> None:
    now = time.monotonic()
    with _push_ok_lock:
        global _push_ok_total
        _push_ok_total += 1
        _push_ok_times.append(now)
        cutoff = now - 60.0
        while _push_ok_times and _push_ok_times[0] < cutoff:
            _push_ok_times.popleft()

def _record_push_fail() -> None:
    now = time.monotonic()
    with _push_ok_lock:
        global _push_fail_total
        _push_fail_total += 1
        _push_fail_times.append(now)
        cutoff = now - 60.0
        while _push_fail_times and _push_fail_times[0] < cutoff:
            _push_fail_times.popleft()

def _push_ok_last_60s() -> int:
    now = time.monotonic()
    cutoff = now - 60.0
    with _push_ok_lock:
        while _push_ok_times and _push_ok_times[0] < cutoff:
            _push_ok_times.popleft()
        return len(_push_ok_times)

def _push_fail_last_60s() -> int:
    now = time.monotonic()
    cutoff = now - 60.0
    with _push_ok_lock:
        while _push_fail_times and _push_fail_times[0] < cutoff:
            _push_fail_times.popleft()
        return len(_push_fail_times)

def _push_totals() -> tuple[int, int]:
    with _push_ok_lock:
        return _push_ok_total, _push_fail_total

def _pushes_per_min_since_start() -> float:
    if _run_start_mono is None:
        return 0.0
    elapsed_s = max(1e-6, time.monotonic() - _run_start_mono)
    mins = elapsed_s / 60.0
    ok_total, fail_total = _push_totals()
    return (ok_total + fail_total) / max(1e-6, mins)

def _progress_reset() -> None:
    global _progress_index_crates, _progress_versions_queued, _progress_versions_done
    global _progress_ok, _progress_skip, _progress_fail
    global _progress_scan_complete, _progress_total
    with _progress_lock:
        _progress_index_crates = 0
        _progress_versions_queued = 0
        _progress_versions_done = 0
        _progress_ok = 0
        _progress_skip = 0
        _progress_fail = 0
        _progress_scan_complete = False
        _progress_total = None

def _progress_note_index_crate() -> None:
    global _progress_index_crates
    with _progress_lock:
        _progress_index_crates += 1

def _progress_note_queued(n: int = 1) -> None:
    global _progress_versions_queued
    with _progress_lock:
        _progress_versions_queued += n

def _progress_note_result(status: str) -> None:
    global _progress_versions_done, _progress_ok, _progress_skip, _progress_fail
    with _progress_lock:
        _progress_versions_done += 1
        if status == "ok":
            _progress_ok += 1
        elif status == "skip":
            _progress_skip += 1
        else:
            _progress_fail += 1

def _progress_mark_scan_complete() -> None:
    global _progress_scan_complete, _progress_total
    with _progress_lock:
        _progress_scan_complete = True
        _progress_total = _progress_versions_queued

def _progress_set_total(total: int) -> None:
    global _progress_scan_complete, _progress_total, _progress_versions_queued
    with _progress_lock:
        _progress_scan_complete = True
        _progress_total = total
        _progress_versions_queued = total

def _format_eta(done: int, total: int | None) -> str:
    if _run_start_mono is None or done <= 0 or not total or total <= done:
        return "eta=?"
    elapsed = max(1e-6, time.monotonic() - _run_start_mono)
    rate = done / elapsed
    remain = (total - done) / max(1e-9, rate)
    if remain < 60:
        return f"eta={remain:.0f}s"
    if remain < 3600:
        return f"eta={remain / 60:.1f}m"
    return f"eta={remain / 3600:.1f}h"

def _format_progress_bar(done: int, total: int | None, width: int = 30) -> str:
    """ASCII progress bar, e.g. [############--------------]  40.0%"""
    if not total or total <= 0:
        return f"[{'-' * width}]   ?.??%"
    frac = min(1.0, max(0.0, done / total))
    filled = int(round(width * frac))
    if done > 0 and filled == 0:
        filled = 1
    filled = min(width, filled)
    bar = "#" * filled + "-" * (width - filled)
    return f"[{bar}] {frac * 100.0:5.1f}%"

def _format_progress_line() -> str:
    with _progress_lock:
        crates = _progress_index_crates
        queued = _progress_versions_queued
        done = _progress_versions_done
        ok_n = _progress_ok
        skip_n = _progress_skip
        fail_n = _progress_fail
        scan_done = _progress_scan_complete
        total = _progress_total
        jobs = _progress_jobs
    # Denominator = final total after scan, else versions discovered so far (grows toward ~2M).
    denom = total if (total and total > 0) else (queued if queued > 0 else None)
    pending = max(0, queued - done)
    qdepth = 0
    if _work_queue is not None:
        try:
            qdepth = _work_queue.qsize()
        except NotImplementedError:
            qdepth = pending
    bar = _format_progress_bar(done, denom)
    scan_tag = "scan=done" if scan_done else "scan=running"
    count_s = f"{done}/{denom}" if denom else f"done={done}"
    return (
        f"progress: {bar}  {count_s}  "
        f"jobs={jobs} ok={ok_n} skip={skip_n} fail={fail_n} "
        f"queue={qdepth} crates={crates} {scan_tag}  "
        f"{_format_eta(done, denom)}"
    )

def _clear_status_block_locked() -> None:
    global _status_block_active, _status_block_last_lens
    if not _status_block_active:
        return
    n = len(_status_block_last_lines) or 1
    # Move to the first line of the block.
    if n > 1:
        sys.stderr.write(f"\x1b[{n-1}F")
    # Clear each line and move down (except after the last line).
    for i in range(n):
        sys.stderr.write("\x1b[2K\r")
        if i != n - 1:
            sys.stderr.write("\x1b[1E")
    # Move back to the first line (where logs should print).
    if n > 1:
        sys.stderr.write(f"\x1b[{n-1}F")
    sys.stderr.flush()
    _status_block_active = False

def _render_status_block_locked(lines: list[str]) -> None:
    global _status_block_last_lines, _status_block_last_lens, _status_block_active
    if not lines:
        return
    # Clear previous block, then paint the new one.
    if _status_block_active:
        _clear_status_block_locked()
    _status_block_last_lines = lines
    _status_block_last_lens = [len(s) for s in lines]
    # Print without trailing newline so the cursor stays on the block.
    for i, s in enumerate(lines):
        sys.stderr.write("\x1b[2K\r" + s)
        if i != len(lines) - 1:
            sys.stderr.write("\n")
    sys.stderr.flush()
    _status_block_active = True

def _format_status_block() -> list[str]:
    d, x, w, p = _stage_counts()
    ok60 = _push_ok_last_60s()
    fail60 = _push_fail_last_60s()
    ok_total, fail_total = _push_totals()
    ppm = _pushes_per_min_since_start()
    with _progress_lock:
        done = _progress_versions_done
        queued = _progress_versions_queued
        total = _progress_total
        ok_n = _progress_ok
        skip_n = _progress_skip
        fail_n = _progress_fail
        crates = _progress_index_crates
        scan_done = _progress_scan_complete
        jobs = _progress_jobs
    # Growing denominator while index scan runs; locks to final total when scan completes.
    denom = total if (total and total > 0) else (queued if queued > 0 else None)
    qdepth = 0
    if _work_queue is not None:
        try:
            qdepth = _work_queue.qsize()
        except NotImplementedError:
            qdepth = max(0, queued - done)
    if denom:
        head = f"progress: {_format_progress_bar(done, denom)}  {done}/{denom}  {_format_eta(done, denom)}"
    else:
        head = f"progress: {_format_progress_bar(0, None)}  scanning index..."
    scan_line = (
        f"scan: crates={crates} versions_found={queued} "
        f"status={'done' if scan_done else 'running (denom grows until full index walk)'}"
    )
    return [
        head,
        scan_line,
        f"config: jobs={jobs} queue_depth={qdepth}",
        f"counts: ok={ok_n} skip={skip_n} fail={fail_n} done={done}",
        f"status: downloading={d} extracting={x} waiting_push={w} pushing={p}",
        (
            f"push: ok_60s={ok60} fail_60s={fail60} "
            f"ok_total={ok_total} fail_total={fail_total} "
            f"per_min={ppm:.2f}"
        ),
    ]

def _with_stage(stage_set: set[str], label: str):
    class _Ctx:
        def __enter__(self):
            with _stage_state_lock:
                stage_set.add(label)
            return self
        def __exit__(self, exc_type, exc, tb):
            with _stage_state_lock:
                stage_set.discard(label)
            return False
    return _Ctx()

def _stage_counts() -> tuple[int, int, int, int]:
    with _stage_state_lock, _download_state_lock:
        return (
            len(_active_downloads),
            len(_active_extracts),
            len(_waiting_push),
            len(_active_pushes),
        )

def _heartbeat_thread(stop_evt: threading.Event) -> None:
    # Periodically refresh the sticky progress footer (always at bottom of stderr).
    # Also paints once immediately so the footer exists before the first interval elapses.
    while True:
        lines = _format_status_block()
        with _print_lock:
            if STATUS_STICKY:
                _render_status_block_locked(lines)
            else:
                print("\n".join(lines), file=sys.stderr, flush=True)
        if stop_evt.wait(max(0.5, float(STATUS_HEARTBEAT_INTERVAL_S))):
            break
    if STATUS_STICKY:
        with _print_lock:
            _clear_status_block_locked()
            # Final snapshot as normal lines so the last state remains in scrollback.
            print("\n".join(_format_status_block()), file=sys.stderr, flush=True)

def _log(level: str, msg: str) -> None:
    # Standardized, low-noise logging. Use --verbose for command outputs.
    # Always write to stderr so sticky status (also on stderr) and logs share one stream.
    if level == "INFO":
        c = BLUE
    elif level == "WARN":
        c = RED
    elif level == "OK":
        c = GREEN
    else:
        c = ""
    prefix = f"[{level}]"
    line = f"{c}{prefix}{RESET} {msg}" if c else f"{prefix} {msg}"
    with _print_lock:
        if STATUS_STICKY:
            _clear_status_block_locked()
        print(line, file=sys.stderr, flush=True)
        if STATUS_STICKY:
            # Re-paint with fresh progress so OK/INFO lines don't leave a stale block.
            _render_status_block_locked(_format_status_block())

def info(msg: str) -> None:
    _log("INFO", msg)

def warn(msg: str) -> None:
    _log("WARN", msg)

def ok(msg: str) -> None:
    _log("OK", msg)

def _fmt_repo(crate_name: str, version: str) -> str:
    return f"{crate_name}@{version}"

def ensure_directory(path):
    # Create a directory if it doesn't exist
    if not os.path.exists(path):
        os.makedirs(path)
        if VERBOSE:
            info(f"Created directory: {path}")

def crates_io_index_rel_path(crate_name: str) -> str:
    """
    Return crates.io index relative path segments for a crate name.

    See: https://doc.rust-lang.org/cargo/reference/registries.html#index-format
    Examples:
      - "a"      -> "1/a"
      - "ab"     -> "2/ab"
      - "abc"    -> "3/a/abc"
      - "tokio"  -> "to/ki/tokio"
    """
    name = crate_name.strip()
    if not name:
        raise ValueError("crate_name is empty")
    n = len(name)
    if n == 1:
        return f"1/{name}"
    if n == 2:
        return f"2/{name}"
    if n == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[0:2]}/{name[2:4]}/{name}"

def crates_io_index_file_path(index_root: str, crate_name: str) -> str:
    # The index "path" is also the on-disk file path inside a crates.io-index checkout.
    return os.path.join(index_root, crates_io_index_rel_path(crate_name))

def mega_third_party_crates_rel_path(crate_name: str, crate_version: str) -> str:
    # Target layout: third-party/rust/crates/<index-path>/<version>/
    # Note: crates.io index path already ends with the crate name (e.g. to/ki/tokio),
    # so do NOT append crate_name again.
    return (
        "third-party/rust/crates/"
        + crates_io_index_rel_path(crate_name)
        + f"/{crate_version}"
    )

def _try_remove_file(path: str) -> None:
    try:
        if os.path.exists(path):
            os.remove(path)
    except Exception as e:
        warn(f"Failed to remove file {path}: {e}")

def _crate_file_seems_valid(crate_path: str) -> bool:
    # Fast integrity check to avoid reusing truncated/invalid downloads.
    # We intentionally only validate "can we open and list members".
    try:
        if not os.path.exists(crate_path):
            return False
        if os.path.getsize(crate_path) <= 0:
            return False
        with tarfile.open(crate_path, 'r:gz') as tar:
            members = tar.getmembers()
            return bool(members)
    except Exception:
        return False

def _human_bytes(n: float) -> str:
    units = ["B", "KiB", "MiB", "GiB", "TiB"]
    v = float(n)
    for u in units:
        if abs(v) < 1024.0 or u == units[-1]:
            if u == "B":
                return f"{int(v)} {u}"
            return f"{v:.1f} {u}"
        v /= 1024.0
    return f"{v:.1f} TiB"

def _download_with_progress(url: str, dest_path: str, *, label: str) -> bool:
    tmp_path = dest_path + ".part"
    _try_remove_file(tmp_path)
    start = time.monotonic()
    last_print = start
    bytes_done = 0
    total = None

    with _download_state_lock:
        _active_downloads.add(label)
        active_n = len(_active_downloads)

    try:
        req = urllib.request.Request(url, headers={"User-Agent": "crates-sync/1.0"})
        with urllib.request.urlopen(req) as resp:
            try:
                cl = resp.headers.get("Content-Length")
                if cl:
                    total = int(cl)
            except Exception:
                total = None

            with open(tmp_path, "wb") as f:
                while True:
                    chunk = resp.read(1024 * 256)
                    if not chunk:
                        break
                    f.write(chunk)
                    bytes_done += len(chunk)

                    if DOWNLOAD_PROGRESS:
                        now = time.monotonic()
                        if now - last_print >= max(0.2, float(DOWNLOAD_PROGRESS_INTERVAL_S)):
                            elapsed = max(1e-6, now - start)
                            rate = bytes_done / elapsed
                            if total:
                                pct = (bytes_done / total) * 100.0
                                info(
                                    f"downloading ({active_n} active) {label}: "
                                    f"{_human_bytes(bytes_done)}/{_human_bytes(total)} "
                                    f"({pct:.1f}%) at {_human_bytes(rate)}/s"
                                )
                            else:
                                info(
                                    f"downloading ({active_n} active) {label}: "
                                    f"{_human_bytes(bytes_done)} at {_human_bytes(rate)}/s"
                                )
                            last_print = now

        os.replace(tmp_path, dest_path)
        if DOWNLOAD_PROGRESS:
            elapsed = max(1e-6, time.monotonic() - start)
            rate = bytes_done / elapsed
            if total:
                info(
                    f"downloaded ({active_n} active) {label}: "
                    f"{_human_bytes(bytes_done)}/{_human_bytes(total)} "
                    f"in {elapsed:.1f}s ({_human_bytes(rate)}/s)"
                )
            else:
                info(
                    f"downloaded ({active_n} active) {label}: "
                    f"{_human_bytes(bytes_done)} in {elapsed:.1f}s ({_human_bytes(rate)}/s)"
                )
        return True
    except Exception as e:
        warn(f"{label} download failed: {e}")
        _try_remove_file(tmp_path)
        _try_remove_file(dest_path)
        return False
    finally:
        with _download_state_lock:
            _active_downloads.discard(label)

def check_and_download_crate(crates_dir, crate_name, crate_version, dl_base_url) -> str | None:
    # Construct the filename and path for the crate
    crate_filename = f"{crate_name}-{crate_version}.crate"
    crate_path = os.path.join(crates_dir, crate_name, crate_filename)
    ensure_directory(os.path.dirname(crate_path))  # Ensure the directory exists

    download_url = f"{dl_base_url}/{crate_name}/{crate_filename}"
    label = _fmt_repo(crate_name, crate_version)

    def download_once() -> bool:
        if VERBOSE:
            info(f"{label} download {download_url}")
        return _download_with_progress(download_url, crate_path, label=label)

    # If we have a cached crate, validate it before reusing.
    if os.path.exists(crate_path) and not _crate_file_seems_valid(crate_path):
        warn(f"{label} cached .crate appears invalid; deleting and re-downloading")
        _try_remove_file(crate_path)

    # Download if missing, then validate; if invalid, retry once.
    if not os.path.exists(crate_path):
        if not download_once():
            return None
        if not _crate_file_seems_valid(crate_path):
            warn(f"{label} downloaded .crate appears invalid; retrying once")
            _try_remove_file(crate_path)
            if not download_once():
                return None
            if not _crate_file_seems_valid(crate_path):
                warn(f"{label} downloaded .crate still invalid after retry")
                _try_remove_file(crate_path)
                return None

    return crate_path

def run_git_command(repo_path, command, *, check: bool = True, log_on_error: bool = True):
    # Run a git command in the specified repository
    try:
        result = subprocess.run(command, cwd=repo_path, check=check, capture_output=True, text=True)
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        if log_on_error:
            warn(f"Git command failed: {e}")
            if VERBOSE:
                out = (e.stdout or "").strip()
                err = (e.stderr or "").strip()
                if out:
                    warn(f"stdout: {out}")
                if err:
                    warn(f"stderr: {err}")
        return None

def ensure_git_remote(repo_path: str, remote_name: str, remote_url: str) -> None:
    existing = subprocess.run(
        ["git", "remote", "get-url", remote_name],
        cwd=repo_path,
        capture_output=True,
        text=True,
    )
    if existing.returncode == 0:
        cur = (existing.stdout or "").strip()
        if cur != remote_url:
            run_git_command(repo_path, ["git", "remote", "set-url", remote_name, remote_url])
    else:
        run_git_command(repo_path, ["git", "remote", "add", remote_name, remote_url])

def git_bearer_auth_extra_header(token: str) -> str:
    return f"Authorization: Bearer {token}"

def maybe_wrap_git_with_bearer(cmd: list[str], token: str | None) -> list[str]:
    if not token:
        return cmd
    header = git_bearer_auth_extra_header(token)
    # Use per-command config; do not store credentials in remote URL or global git config.
    return ["git", "-c", f"http.extraHeader={header}", *cmd[1:]]

def _git_has_any_commit(repo_path: str) -> bool:
    res = subprocess.run(
        ["git", "rev-parse", "--verify", "HEAD"],
        cwd=repo_path,
        capture_output=True,
        text=True,
    )
    return res.returncode == 0

def ensure_remote_and_push_existing(
    repo_path: str,
    rel: str,
    git_base_url: str,
    *,
    crate_name: str,
    version: str,
    commit_signoff: bool,
    auth_token: str | None,
    force: bool,
    force_with_lease: bool,
    push_sema: threading.Semaphore,
) -> bool:
    remote_url = f"{git_base_url.rstrip('/')}/{rel}"
    info(f"{_fmt_repo(crate_name, version)} push {rel}")
    if VERBOSE:
        info(f"remote: {remote_url}")
    # Make sure we're on main (and create it if missing)
    run_git_command(repo_path, ['git', 'checkout', '-B', 'main'])

    # If this repo has no commits (e.g. leftover from a previous dry-run),
    # create an initial import commit so `git push main` has a ref.
    if not _git_has_any_commit(repo_path):
        run_git_command(repo_path, ['git', 'add', '-A'])
        msg = f"Import {crate_name} {version}"
        cmd = ['git', 'commit', '--allow-empty', '-m', msg]
        if commit_signoff:
            cmd.append('-s')
        run_git_command(repo_path, cmd)

    ensure_git_remote(repo_path, "mega", remote_url)

    label = _fmt_repo(crate_name, version)

    # Measure time waiting for a push slot, then hold the slot for the whole push.
    with _with_stage(_waiting_push, label):
        t_wait0 = time.monotonic()
        push_sema.acquire()
        waited = time.monotonic() - t_wait0
    if waited >= 1.0:
        info(f"{label} waited {waited:.1f}s for push slot")

    try:
        with _with_stage(_active_pushes, label):
            t_push0 = time.monotonic()
            push_args = ['git', 'push', '-u', 'mega', 'main']
            if force_with_lease:
                push_args.insert(2, '--force-with-lease')
            elif force:
                push_args.insert(2, '--force')
            push_cmd = maybe_wrap_git_with_bearer(push_args, auth_token)
            res = run_git_command(repo_path, push_cmd, log_on_error=True)
            dt_push = time.monotonic() - t_push0
        if dt_push >= 5.0:
            info(f"{label} push finished in {dt_push:.1f}s")
    finally:
        push_sema.release()
    if res is None:
        # Keep repo on disk for troubleshooting
        _record_push_fail()
        return False
    _record_push_ok()

    # On success, remove local repo directory to save disk space.
    try:
        shutil.rmtree(repo_path)
        if VERBOSE:
            info(f"Removed local repo (existing): {repo_path}")
    except Exception as e:
        warn(f"Failed to remove local repo {repo_path}: {e}")
    return True

def init_git_repo(repo_path):
    # Initialize a git repository if it doesn't exist
    new_repo = not os.path.exists(os.path.join(repo_path, '.git'))
    if new_repo:
        run_git_command(repo_path, ['git', 'init', '-b', 'main'])
        if VERBOSE:
            info(f"Initialized git repository in {repo_path}")

        # Avoid requiring GPG on fresh machines
        run_git_command(repo_path, ['git', 'config', 'commit.gpgsign', 'false'])
    # Provide a default identity so commits work even when global git config is empty.
    # This is applied both for new repos and existing ones created by older runs.
    run_git_command(repo_path, ['git', 'config', 'user.name', 'crates-sync'])
    run_git_command(repo_path, ['git', 'config', 'user.email', 'crates-sync@example.com'])

def extract_crate(crate_path, extract_path):
    def is_within_directory(directory, target):
        # Check if a path is within a directory (for security)
        abs_directory = os.path.abspath(directory)
        abs_target = os.path.abspath(target)
        prefix = os.path.commonprefix([abs_directory, abs_target])
        return prefix == abs_directory

    def safe_extract(tar, path=".", members=None, *, numeric_owner=False):
        # Safely extract files from a tar archive
        for member in tar.getmembers():
            member_path = os.path.join(path, member.name)
            if not is_within_directory(path, member_path):
                raise Exception("Attempted Path Traversal in Tar File")

        def filter_member(tarinfo, filterpath):
            # Filter function to ensure extracted files are within the target directory
            if is_within_directory(path, os.path.join(filterpath, tarinfo.name)):
                return tarinfo
            else:
                return None

        tar.extractall(path, members, numeric_owner=numeric_owner, filter=filter_member)

    try:
        if not os.path.exists(crate_path):
            warn(f"Crate file missing {crate_path}. Skipping extraction.")
            return False
        with tarfile.open(crate_path, 'r:gz') as tar:
            if not tar.getmembers():
                warn(f"Empty crate file {crate_path}. Skipping extraction.")
                return False

            # Create a temporary directory for extraction
            temp_extract_path = extract_path + "_temp"
            ensure_directory(temp_extract_path)

            # Extract to the temporary directory
            safe_extract(tar, temp_extract_path)

            # Move contents from the nested directory to the target directory
            nested_dir = os.path.join(temp_extract_path, os.listdir(temp_extract_path)[0])
            for item in os.listdir(nested_dir):
                shutil.move(os.path.join(nested_dir, item), extract_path)

            # Remove the temporary directory
            shutil.rmtree(temp_extract_path)

        print(f"Extracted version to {extract_path}")
        return True
    except tarfile.ReadError:
        warn(f"Failed to read crate file {crate_path}. Skipping extraction.")
        return False
    except Exception as e:
        # Covers truncated gzip streams, partial downloads, and unexpected tar errors.
        warn(f"Failed to extract crate file {crate_path}: {e}")
        return False

def process_crate_version(
    num: int,
    crate_name: str,
    version: str,
    crate_path: str,
    git_repos_dir: str,
    git_base_url: str,
    *,
    commit_signoff: bool,
    dry_run: bool,
    auth_token: str | None,
    force: bool,
    force_with_lease: bool,
    push_sema: threading.Semaphore,
    keep_crate_cache: bool = False,
) -> bool:
    if VERBOSE:
        info(f"Started {crate_name} at {datetime.now()}")

    # Process a specific version of a crate
    rel = mega_third_party_crates_rel_path(crate_name, version)
    repo_path = os.path.join(git_repos_dir, rel)
    ensure_directory(repo_path)

    label = _fmt_repo(crate_name, version)
    # Extract crate directly to the repo directory
    with _with_stage(_active_extracts, label):
        t0 = time.monotonic()
        ok_extract = extract_crate(crate_path, repo_path)
        dt_extract = time.monotonic() - t0
    if dt_extract >= 5.0:
        info(f"{label} extract finished in {dt_extract:.1f}s")

    if not ok_extract:
        warn(f"{_fmt_repo(crate_name, version)} skipped: extraction failed")
        # Avoid repeated failures due to a cached truncated/invalid .crate file.
        _try_remove_file(crate_path)
        return False

    # Check for .gitattributes file and remove if it exists
    gitattributes_path = os.path.join(repo_path, '.gitattributes')
    if os.path.exists(gitattributes_path):
        os.remove(gitattributes_path)
        if VERBOSE:
            info(f"Removed .gitattributes file from {repo_path}")

    # Initialize git repo
    init_git_repo(repo_path)

    if dry_run:
        info(f"{_fmt_repo(crate_name, version)} dry-run {rel}")
        return True

    # Add all files to git
    run_git_command(repo_path, ['git', 'add', '.'])

    # Commit changes
    commit_message = f"Import {crate_name} {version}"
    commit_cmd = ['git', 'commit', '--allow-empty', '-m', commit_message]
    if commit_signoff:
        commit_cmd.append('-s')
    run_git_command(repo_path, commit_cmd)

    # Add remote and push (path-style URL, no .git suffix needed)
    remote_url = f"{git_base_url.rstrip('/')}/{rel}"
    ensure_git_remote(repo_path, "mega", remote_url)

    # Push to remote
    with _with_stage(_waiting_push, label):
        t_wait0 = time.monotonic()
        push_sema.acquire()
        waited = time.monotonic() - t_wait0
    if waited >= 1.0:
        info(f"{label} waited {waited:.1f}s for push slot")

    try:
        with _with_stage(_active_pushes, label):
            t_push0 = time.monotonic()
            push_args = ['git', 'push', '-u', 'mega', 'main']
            if force_with_lease:
                push_args.insert(2, '--force-with-lease')
            elif force:
                push_args.insert(2, '--force')
            push_cmd = maybe_wrap_git_with_bearer(push_args, auth_token)
            push_result = run_git_command(repo_path, push_cmd)
            dt_push = time.monotonic() - t_push0
        if dt_push >= 5.0:
            info(f"{label} push finished in {dt_push:.1f}s")
    finally:
        push_sema.release()
    if push_result is None:
        warn(f"{_fmt_repo(crate_name, version)} push failed")
        _record_push_fail()
        return False

    _record_push_ok()
    # On success, remove local repo directory to save disk space.
    try:
        shutil.rmtree(repo_path)
        if VERBOSE:
            info(f"Removed local repo: {repo_path}")
    except Exception as e:
        warn(f"Failed to remove local repo {repo_path}: {e}")
    # Optionally drop cached .crate (keep when sharing a host freighter cache).
    if not keep_crate_cache:
        try:
            if os.path.exists(crate_path):
                os.remove(crate_path)
                if VERBOSE:
                    info(f"Removed cached crate file: {crate_path}")
        except Exception as e:
            warn(f"Failed to remove cached crate file {crate_path}: {e}")
    return True

def stream_index_crate_versions(index_path: str, max_versions_per_crate: int):
    """
    Walk crates.io-index and yield (crate_name, versions_to_process) per index file.
    Each file corresponds to one crate; versions are sorted and optionally trimmed
    before yielding so work can start without scanning the whole tree into memory.
    """
    if not os.path.isdir(index_path):
        warn(f"Error: The directory {index_path} does not exist.")
        sys.exit(1)

    files_seen = 0
    for root, dirs, files in os.walk(index_path):
        dirs[:] = [d for d in dirs if d not in [".git", ".github"]]

        for file in files:
            if file.startswith("."):
                continue
            if (root == index_path and file == "config.json") or file == "README.md":
                continue

            full_path = os.path.join(root, file)
            if not os.path.isfile(full_path):
                continue

            crate_name = file
            versions: set[str] = set()
            try:
                with open(full_path, "r", encoding="utf-8") as f:
                    for line in f:
                        line = line.strip()
                        if not line:
                            continue
                        crate_info = json.loads(line)
                        versions.add(crate_info["vers"])
            except UnicodeDecodeError:
                continue
            except Exception as e:
                warn(f"Error processing file {full_path}: {e}")
                continue

            if not versions:
                continue

            vs = sorted(versions)
            if max_versions_per_crate > 0:
                vs = vs[-max_versions_per_crate:]

            _progress_note_index_crate()
            _progress_note_queued(len(vs))
            yield crate_name, vs
            files_seen += 1
            if files_seen % 1000 == 0:
                info(f"Index stream progress: processed {files_seen} crate index files...")

def scan_selected_crates_index(index_path: str, crate_names: list[str]):
    """
    Read only the index files for the specified crates.
    This avoids a full os.walk over crates.io-index for small-scale tests.
    """
    crates = defaultdict(set)
    for name in crate_names:
        name = name.strip()
        if not name:
            continue
        fp = crates_io_index_file_path(index_path, name)
        if not os.path.isfile(fp):
            warn(f"Crate '{name}' index file not found: {fp}")
            continue
        try:
            with open(fp, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    crate_info = json.loads(line)
                    crates[crate_info["name"]].add(crate_info["vers"])
        except Exception as e:
            warn(f"Error processing crate index file {fp}: {e}")
    return crates

def list_all_crate_names_from_index_tree(index_path: str) -> list[str]:
    """
    List all crate names by walking the index tree, without parsing JSON lines.
    This is significantly cheaper than scanning every file's contents.
    """
    names: list[str] = []
    files_seen = 0
    for root, dirs, files in os.walk(index_path):
        dirs[:] = [d for d in dirs if d not in ['.git', '.github']]
        for file in files:
            if file.startswith("."):
                continue
            if (root == index_path and file == "config.json") or file == "README.md":
                continue
            full_path = os.path.join(root, file)
            if not os.path.isfile(full_path):
                continue
            # In crates.io-index, the filename is the crate name (for all lengths).
            names.append(file)
            files_seen += 1
            if files_seen % 20000 == 0:
                info(f"Crate-name scan progress: visited {files_seen} index files...")
    # De-dup and keep stable order
    return sorted(set(names))

def load_or_build_crate_name_cache(index_path: str, cache_path: str) -> list[str]:
    cache = Path(cache_path)
    if cache.is_file():
        data = cache.read_text(encoding="utf-8", errors="replace").splitlines()
        names = [x.strip() for x in data if x.strip() and not x.strip().startswith("#")]
        if names:
            return names

    info(f"Building crate name cache (one-time): {cache}")
    names = list_all_crate_names_from_index_tree(index_path)
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text("\n".join(names) + "\n", encoding="utf-8")
    return names


def load_manifest(manifest_path: str) -> Dict[Tuple[str, str], dict]:
    """Load import status manifest from a JSONL file.

    Keyed by (crate_name, version) -> record dict.
    """
    manifest: Dict[Tuple[str, str], dict] = {}
    p = Path(manifest_path)
    if not p.is_file():
        return manifest
    for line in p.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
            key = (rec.get("crate") or "", rec.get("version") or "")
            if key[0] and key[1]:
                manifest[key] = rec
        except Exception:
            # Ignore malformed lines, keep going
            continue
    return manifest


def write_manifest(manifest_path: str, manifest: Dict[Tuple[str, str], dict]) -> None:
    """Rewrite manifest file from in-memory mapping (compact, one line per key)."""
    lines = [json.dumps(rec, sort_keys=True) for rec in manifest.values()]
    p = Path(manifest_path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")


def append_manifest_record(manifest_path: str, rec: dict) -> None:
    """Append one manifest line so progress survives crashes before run end."""
    p = Path(manifest_path)
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.open("a", encoding="utf-8") as f:
        f.write(json.dumps(rec, sort_keys=True) + "\n")

def scan_and_process_crates(
    index_path: str,
    crates_dir: str,
    git_repos_dir: str,
    git_base_url: str,
    *,
    limit_crates: int,
    only_crates: list[str],
    max_versions_per_crate: int,
    commit_signoff: bool,
    dry_run: bool,
    repush_existing: bool,
    auth_token: str | None,
    force: bool,
    force_with_lease: bool,
    crate_name_cache: str,
    jobs: int,
    manifest: Dict[Tuple[str, str], dict],
    manifest_path: str,
    reimport_ok: bool,
    keep_crate_cache: bool = False,
) -> tuple[int, int, int]:
    global _progress_jobs, _work_queue
    info("Scanning crates.io index...")
    _progress_reset()

    stop_evt = threading.Event()
    hb_thread = None
    if STATUS_HEARTBEAT:
        hb_thread = threading.Thread(target=_heartbeat_thread, args=(stop_evt,), daemon=True)
        hb_thread.start()

    try:
        # Read the config.json to get the dl base URL (needed before any processing)
        config_path = os.path.join(index_path, "config.json")
        try:
            with open(config_path, "r") as config_file:
                config = json.load(config_file)
                dl_base_url = config.get("dl")
                if not dl_base_url:
                    warn("Error: 'dl' key not found in config.json")
                    sys.exit(1)
        except Exception as e:
            warn(f"Error reading config.json: {e}")
            sys.exit(1)

        use_streaming_full_index = not only_crates and not (limit_crates and limit_crates > 0)

        if use_streaming_full_index:
            info(
                "Streaming index in parallel: producer walks full crates.io-index "
                "while workers import/push (denominator grows with scan ~2M versions)."
            )
        elif only_crates:
            crates = scan_selected_crates_index(index_path, only_crates)
            info(f"Found {len(crates)} crates.")
            crates_items = list(crates.items())
            allow = {c.strip() for c in only_crates if c.strip()}
            crates_items = [(n, v) for (n, v) in crates_items if n in allow]
            info(f"Filtered to {len(crates_items)} crates via --crate.")
        else:
            names = load_or_build_crate_name_cache(index_path, crate_name_cache)
            random.shuffle(names)
            picked = names[:limit_crates]
            info(f"Sampling {len(picked)} crates via --limit-crates (no full content scan).")
            crates = scan_selected_crates_index(index_path, picked)
            info(f"Loaded {len(crates)} crates' versions.")
            crates_items = list(crates.items())
            if VERBOSE:
                info("Shuffling crates list...")
            random.shuffle(crates_items)

        push_sema = threading.BoundedSemaphore(max(1, int(jobs)))
        succeeded = 0
        failed = 0
        skipped = 0
        lock = threading.Lock()
        _progress_jobs = max(1, int(jobs))
        info(
            f"Config: jobs={_progress_jobs} "
            f"max_versions_per_crate={max_versions_per_crate} "
            f"keep_crate_cache={keep_crate_cache} "
            f"reimport_ok={reimport_ok} dry_run={dry_run}"
        )

        def process_one(crate_name: str, v: str) -> tuple[str, str, str]:
            key = (crate_name, v)
            rec = manifest.get(key)
            # If already successfully imported, skip unless --reimport-ok (git --force does not disable this).
            if rec and rec.get("status") == "ok" and not reimport_ok:
                if VERBOSE:
                    info(f"{_fmt_repo(crate_name, v)} already imported; skipping (manifest)")
                return ("skip", crate_name, v)

            rel = mega_third_party_crates_rel_path(crate_name, v)
            repo_path = os.path.join(git_repos_dir, rel)

            # Existing repo path
            if os.path.exists(repo_path) and os.path.exists(os.path.join(repo_path, ".git")):
                if repush_existing and not dry_run:
                    ok_push = ensure_remote_and_push_existing(
                        repo_path,
                        rel,
                        git_base_url,
                        crate_name=crate_name,
                        version=v,
                        commit_signoff=commit_signoff,
                        auth_token=auth_token,
                        force=force,
                        force_with_lease=force_with_lease,
                        push_sema=push_sema,
                    )
                    return ("ok" if ok_push else "fail", crate_name, v)
                else:
                    if VERBOSE:
                        info(f"{_fmt_repo(crate_name, v)} exists; skipping")
                    return ("skip", crate_name, v)

            crate_path = check_and_download_crate(crates_dir, crate_name, v, dl_base_url)
            if crate_path is None:
                return ("fail", crate_name, v)
            try:
                ok_done = process_crate_version(
                    0,
                    crate_name,
                    v,
                    crate_path,
                    git_repos_dir,
                    git_base_url,
                    commit_signoff=commit_signoff,
                    dry_run=dry_run,
                    auth_token=auth_token,
                    force=force,
                    force_with_lease=force_with_lease,
                    push_sema=push_sema,
                    keep_crate_cache=keep_crate_cache,
                )
                return ("ok" if ok_done else "fail", crate_name, v)
            except Exception as e:
                warn(f"{_fmt_repo(crate_name, v)} failed: {e}")
                return ("fail", crate_name, v)

        def record_result_status(status: str, c_name: str, v: str) -> None:
            nonlocal succeeded, failed, skipped
            _progress_note_result(status)
            # Print after progress counters update so sticky footer shows the new totals.
            if status == "ok":
                ok(f"{_fmt_repo(c_name, v)} pushed")
            with lock:
                if status == "ok":
                    succeeded += 1
                elif status == "skip":
                    skipped += 1
                else:
                    failed += 1
                key = (c_name, v)
                rel = mega_third_party_crates_rel_path(c_name, v)
                rec = {
                    "crate": c_name,
                    "version": v,
                    "status": status,
                    "remote": f"{git_base_url.rstrip('/')}/{rel}",
                    "last_import_time": datetime.now(timezone.utc).isoformat(),
                }
                manifest[key] = rec
                append_manifest_record(manifest_path, rec)

        def record_result(fut) -> None:
            status, c_name, v = fut.result()
            record_result_status(status, c_name, v)

        n_jobs = max(1, int(jobs))
        if use_streaming_full_index:
            # Producer (index scan) runs at full speed into a large work queue while
            # workers download/push in parallel. Denominator (versions_found) grows
            # with the scan toward the full crates.io size (~2M versions), not with
            # the tiny in-flight window that previously stalled the walk.
            #
            # Queue holds (name, version) only — ~2M entries is acceptable within Job memory.
            work_q: queue.Queue = queue.Queue(maxsize=0)
            _work_queue = work_q
            scan_error: list[BaseException] = []

            def index_producer() -> None:
                try:
                    for crate_name, versions in stream_index_crate_versions(
                        index_path, max_versions_per_crate
                    ):
                        for v in versions:
                            work_q.put((crate_name, v))
                    _progress_mark_scan_complete()
                    info(
                        f"Index scan complete: {_progress_index_crates} crates, "
                        f"{_progress_versions_queued} versions found; workers draining queue..."
                    )
                except BaseException as e:
                    scan_error.append(e)
                    warn(f"Index scan failed: {e}")
                finally:
                    for _ in range(n_jobs):
                        work_q.put(None)

            def worker_loop() -> None:
                while True:
                    item = work_q.get()
                    try:
                        if item is None:
                            return
                        crate_name, v = item
                        status, c_name, ver = process_one(crate_name, v)
                        record_result_status(status, c_name, ver)
                    finally:
                        work_q.task_done()

            info(
                "Parallel mode: index scan producer + "
                f"{n_jobs} import workers (queue unbounded for version descriptors)."
            )
            producer = threading.Thread(
                target=index_producer, name="crates-index-scan", daemon=True
            )
            workers = [
                threading.Thread(
                    target=worker_loop, name=f"crates-worker-{i}", daemon=True
                )
                for i in range(n_jobs)
            ]
            producer.start()
            for t in workers:
                t.start()
            producer.join()
            for t in workers:
                t.join()
            _work_queue = None
            if scan_error:
                raise scan_error[0]
        else:
            _work_queue = None
            tasks: list[tuple[str, str]] = []
            for crate_name, versions in crates_items:
                vs = sorted(versions)
                if max_versions_per_crate > 0:
                    vs = vs[-max_versions_per_crate:]
                for v in vs:
                    tasks.append((crate_name, v))
            _progress_set_total(len(tasks))
            info(f"Starting to process {len(tasks)} crate versions...")
            with ThreadPoolExecutor(max_workers=n_jobs) as ex:
                futures = [ex.submit(process_one, c, v) for c, v in tasks]
                for f in as_completed(futures):
                    record_result(f)

        info(f"Summary: ok={succeeded}, skipped={skipped}, failed={failed}")
        info(_format_progress_line())
        # Compact manifest (dedupe append-only history to one line per crate@version).
        write_manifest(manifest_path, manifest)
        return succeeded, skipped, failed
    finally:
        _work_queue = None
        stop_evt.set()
        if hb_thread is not None:
            hb_thread.join(timeout=max(1.0, float(STATUS_HEARTBEAT_INTERVAL_S) + 1.0))


def main():
    # Record start time for the entire process
    total_start_time = datetime.now()
    info(f"Started at {total_start_time}")
    global _run_start_mono
    _run_start_mono = time.monotonic()

    p = argparse.ArgumentParser(prog="crates-sync.py")
    p.add_argument("--index", required=True, help="Path to a local crates.io-index checkout.")
    p.add_argument("--crates-dir", required=True, help="Directory to cache downloaded .crate files.")
    p.add_argument("--workdir", required=True, help="Directory to stage per-crate git repos.")
    p.add_argument("--git-base-url", required=True, help="Mega git base URL, e.g. https://git.gitmega.com")
    # Crates.io `.crate` sources should not require Git LFS; we intentionally do not configure LFS here.
    p.add_argument("--limit-crates", type=int, default=0, help="Process only N crates (0 = no limit).")
    p.add_argument(
        "--max-versions-per-crate",
        type=int,
        default=1,
        help="Keep only the last N versions per crate (0 = all). Default: 1 (small-scale test).",
    )
    p.add_argument(
        "--crate",
        action="append",
        default=[],
        help="Only process this crate name (can be provided multiple times). Overrides shuffle/limit selection.",
    )
    p.add_argument("--signoff", action="store_true", help="Add -s to git commit.")
    p.add_argument("--dry-run", action="store_true", help="Do everything except git add/commit/push.")
    p.add_argument("--verbose", action="store_true", help="Verbose logs (print git stdout/stderr).")
    p.add_argument(
        "--download-progress",
        action="store_true",
        help="Show download progress/speed (recommended with --verbose).",
    )
    p.add_argument(
        "--download-progress-interval",
        type=float,
        default=1.0,
        help="Seconds between download progress prints (default: 1.0).",
    )
    p.add_argument(
        "--status-heartbeat",
        dest="status_heartbeat",
        action="store_true",
        default=True,
        help="Periodically print a compact status line (active download/extract/push counts). Default: on.",
    )
    p.add_argument(
        "--no-status-heartbeat",
        dest="status_heartbeat",
        action="store_false",
        help="Disable periodic status heartbeat.",
    )
    p.add_argument(
        "--status-interval",
        type=float,
        default=10.0,
        help="Seconds between status heartbeat prints (default: 10).",
    )
    p.add_argument(
        "--status-sticky",
        dest="status_sticky",
        action="store_true",
        default=True,
        help="Render status heartbeat as a single updating line (won't scroll). Default: on.",
    )
    p.add_argument(
        "--no-status-sticky",
        dest="status_sticky",
        action="store_false",
        help="Disable sticky status line (status will print as normal log lines).",
    )
    p.add_argument("--jobs", type=int, default=1, help="Concurrent workers for download/extract/commit (default: 1).")
    p.add_argument(
        "--repush-existing",
        action="store_true",
        help="If target repo already exists in workdir, re-push it instead of skipping (no re-extract).",
    )
    p.add_argument("--force", action="store_true", help="Force-push when pushing (overwrites remote history).")
    p.add_argument(
        "--force-with-lease",
        action="store_true",
        help="Force-push with lease (safer than --force).",
    )
    p.add_argument(
        "--token",
        default=os.environ.get("MEGA_TOKEN", ""),
        help="Bearer token for HTTP auth (or env MEGA_TOKEN).",
    )
    p.add_argument(
        "--crate-name-cache",
        default="",
        help="Path to a cached crate name list (one per line). Used to make --limit-crates fast.",
    )
    p.add_argument(
        "--manifest",
        default="",
        help="Path to an import status manifest (JSONL). Defaults to <workdir>/crates-import-manifest.jsonl.",
    )
    p.add_argument(
        "--reimport-ok",
        action="store_true",
        help="Re-process crate versions even if manifest status is ok (default: skip ok).",
    )
    p.add_argument(
        "--keep-crate-cache",
        action="store_true",
        help="Do not delete downloaded .crate files after a successful push (for shared host caches).",
    )
    args = p.parse_args()

    global VERBOSE
    VERBOSE = bool(args.verbose)
    global DOWNLOAD_PROGRESS, DOWNLOAD_PROGRESS_INTERVAL_S
    DOWNLOAD_PROGRESS = bool(args.download_progress) or VERBOSE
    DOWNLOAD_PROGRESS_INTERVAL_S = float(args.download_progress_interval or 1.0)
    global STATUS_HEARTBEAT, STATUS_HEARTBEAT_INTERVAL_S
    STATUS_HEARTBEAT = bool(args.status_heartbeat)
    STATUS_HEARTBEAT_INTERVAL_S = float(args.status_interval or 15.0)
    global STATUS_STICKY
    STATUS_STICKY = bool(args.status_sticky)

    if args.force and args.force_with_lease:
        warn("Error: --force and --force-with-lease are mutually exclusive.")
        sys.exit(2)

    index_path = str(Path(args.index).resolve())
    crates_dir = str(Path(args.crates_dir).resolve())
    git_repos_dir = str(Path(args.workdir).resolve())

    ensure_directory(crates_dir)
    ensure_directory(git_repos_dir)

    auth_token = args.token.strip() or None
    crate_name_cache = args.crate_name_cache.strip()
    if not crate_name_cache:
        crate_name_cache = str(Path(git_repos_dir) / ".crates-io-index-crate-names.txt")

    manifest_path = args.manifest.strip()
    if not manifest_path:
        manifest_path = str(Path(git_repos_dir) / "crates-import-manifest.jsonl")
    manifest = load_manifest(manifest_path)

    succeeded, skipped, failed = scan_and_process_crates(
        index_path,
        crates_dir,
        git_repos_dir,
        args.git_base_url,
        limit_crates=args.limit_crates,
        only_crates=args.crate,
        max_versions_per_crate=args.max_versions_per_crate,
        commit_signoff=args.signoff,
        dry_run=args.dry_run,
        repush_existing=args.repush_existing,
        auth_token=auth_token,
        force=args.force,
        force_with_lease=args.force_with_lease,
        crate_name_cache=crate_name_cache,
        jobs=args.jobs,
        manifest=manifest,
        manifest_path=manifest_path,
        reimport_ok=args.reimport_ok,
        keep_crate_cache=bool(args.keep_crate_cache),
    )

    # Record end time and calculate duration for the entire process
    total_end_time = datetime.now()
    total_duration = total_end_time - total_start_time
    total_crates = succeeded + skipped + failed
    info(f"Total processed: {total_crates} (ok={succeeded}, skipped={skipped}, failed={failed})")
    info(f"Finished at {total_end_time} (duration {total_duration})")
    if failed > 0:
        sys.exit(1)

if __name__ == "__main__":
    main()  # Run the main function if this script is executed directly
