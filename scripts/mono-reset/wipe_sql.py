#!/usr/bin/env python3
"""Truncate all public Postgres tables except the login/identity keep-list.

MySQL (Campsite) is never touched. Keep-list tables that are missing are skipped.
"""

from __future__ import annotations

import subprocess
import sys

# PG tables that must survive a mono git/monorepo reset.
KEEP_TABLES = frozenset(
    {
        "campsite_member_identity",
        "user_approval_status",
        "access_token",
        "ssh_keys",
        "gpg_key",
        "cla_sign_status",
        "vault",
        "path_check_configs",
        # SeaORM / migration bookkeeping — never truncate
        "seaql_migrations",
    }
)


def run_psql(db_url: str, sql: str, *, tuples_only: bool = False) -> str:
    cmd = ["psql", db_url, "-v", "ON_ERROR_STOP=1", "-At" if tuples_only else "-q"]
    result = subprocess.run(
        cmd,
        input=sql,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"psql failed ({result.returncode}): {result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def list_public_tables(db_url: str) -> list[str]:
    out = run_psql(
        db_url,
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY 1;",
        tuples_only=True,
    )
    return [line.strip() for line in out.splitlines() if line.strip()]


def wipe_public_tables(db_url: str, *, dry_run: bool = False) -> list[str]:
    tables = list_public_tables(db_url)
    if not tables:
        print("No public tables found; nothing to wipe.")
        return []

    keep_present = sorted(t for t in tables if t in KEEP_TABLES)
    wipe = sorted(t for t in tables if t not in KEEP_TABLES)

    print(f"Public tables: {len(tables)}")
    print(f"Keeping ({len(keep_present)}): {', '.join(keep_present) or '(none present)'}")
    missing_keep = sorted(KEEP_TABLES - set(tables) - {"seaql_migrations"})
    if missing_keep:
        print(f"Keep-list missing from DB (ok): {', '.join(missing_keep)}")

    if not wipe:
        print("Nothing to truncate.")
        return []

    print(f"Will TRUNCATE CASCADE ({len(wipe)}): {', '.join(wipe)}")
    if dry_run:
        print("DRY-RUN: skipping TRUNCATE.")
        return wipe

    # Quote identifiers; single statement for one CASCADE graph.
    quoted = ", ".join(f'"{t}"' for t in wipe)
    run_psql(db_url, f"TRUNCATE TABLE {quoted} RESTART IDENTITY CASCADE;")
    print("TRUNCATE complete.")
    return wipe


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--db-url",
        default=None,
        help="Postgres URL (default: MEGA_DATABASE__DB_URL or DATABASE_URL)",
    )
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)

    import os

    db_url = (
        (args.db_url or "").strip()
        or os.environ.get("MEGA_DATABASE__DB_URL", "").strip()
        or os.environ.get("DATABASE_URL", "").strip()
    )
    if not db_url:
        print("Missing --db-url / MEGA_DATABASE__DB_URL / DATABASE_URL", file=sys.stderr)
        return 2

    wipe_public_tables(db_url, dry_run=args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
