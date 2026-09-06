# jupiter-migrate

SeaORM database migrations for Mega, extracted from `jupiter` so day-to-day `cargo check` does not compile migration code unless needed.

## Apply migrations

`mono` enables `jupiter/migrate`. On startup, `Storage::new` calls `jupiter_migrate::apply_migrations` automatically (`jupiter/src/storage/init.rs`). No separate `init` CLI step is required.

After schema migrations, mono HTTP boot may run **application data backfills** (e.g. actor handle → `campsite_user_id`) using Campsite `internal/member_identities` and `data_backfill_ledger`. Deploy campsite-api with `MEGA_INTERNAL_SECRET` before mono with `MEGA_OAUTH__MEGA_INTERNAL_SECRET`.

Crates that need a migrated DB in tests should enable `jupiter/migrate` or `ceres` feature `migrate`.

## Generate a new migration

```bash
cd jupiter-migrate/src/migration
sea-orm-cli migrate generate "your_migration_name"
```

Commit the new file under `jupiter-migrate/src/migration/`.

## Regenerate entities

After schema changes, regenerate callisto entities (adjust connection URL for your DB):

```bash
sea-orm-cli generate entity \
  -u postgres://postgres:postgres@localhost:5432/mono \
  -o jupiter/callisto/src \
  --with-serde both \
  --entity-format dense
```

Review generated diffs in `jupiter/callisto/src/` before committing.

**Do not edit CLI-generated entity files** for polymorphic/link joins or `Model::new` helpers — those live only in `entity_ext/`. Regenerating must overwrite table models cleanly.

**After every regen:**

1. Re-add `pub mod entity_ext;` to `jupiter/callisto/src/mod.rs` (codegen overwrites this file).
2. Keep `sea_orm_active_enums.rs` webhook variant names readable (`ClCreated`, not CLI-mangled names). `rs_type = "Enum"` is correct for SeaORM 2.0 — call sites that need a string use `to_value().value` or `TryFrom<&str>`.
3. Leave `entity_ext/` alone — it owns:
   - `Model::new` helpers and ID utilities
   - Polymorphic / link-based `Relation` + `Related` (no DB FKs), including:
     - `item_labels` / `item_assignees`: dual `belongs_to` on `item_id` → `mega_cl` and `mega_issue`
     - `mega_cl` / `mega_issue`: `has_many` labels (via), assignees, conversations
     - `mega_conversation`: link joins to CL/Issue; `has_many` reactions
     - `reactions`: `belongs_to` conversation
     - `mega_code_review_thread`: `belongs_to` `mega_cl` on `link`
     - `label`: `has_many` `item_labels`

Join call sites that need those relations use `callisto::entity_ext::<table>::Relation`, not the generated entity `Relation`.

## Snapshot entity generation and database gates

The snapshot source identity migration is additive; it does not create a published
namespace or guess historical scope mappings. To reproduce only its entities,
first create a new temporary directory, then run the following with its absolute
path substituted for `<temp>`:

```bash
cargo run -p jupiter-migrate --example snapshot_schema -- <temp>/schema.db
sea-orm-cli generate entity -u sqlite://<temp>/schema.db -o <temp>/entities --tables snapshot_instance,snapshot_source,source_commit_scope,namespace_node --with-serde both --entity-format dense
```

The example rejects existing database files. Review and copy only those four
generated entity files into Callisto; merge their module/prelude registrations
without replacing the existing registries or `entity_ext`. The initial generation
uses sea-orm-cli 2.0.2. SQLite generation verifies the actual migration schema;
PostgreSQL runtime/transaction tests separately check backend compatibility. The
namespace node entity uses `i64`/SQL BIGINT and a timezone-aware timestamp on both
backends. Its payload has a 16 KiB database check as well as storage-layer checks.

`source_commit_scope` indexes a SHA-256 scope key and retains the full UTF-8 path
as data. This avoids placing multi-kilobyte paths in a PostgreSQL btree key. The
storage facade checks the full path on reads and rejects conflicting immutable
attestations. There is no cascading FK from mutable refs or repo paths to proof records.

The forward migration `m20260906_145000_snapshot_utc_timestamps` converts the three
earlier source tables' PostgreSQL `created_at` columns from TIMESTAMP to
TIMESTAMPTZ, matching their generated `DateTimeUtc` models. It explicitly treats
legacy values as UTC and is a no-op on SQLite. This is a compatibility repair,
not a namespace backfill. **Before applying to an already-used draft database,
verify that those legacy values were written in UTC.** A legacy deployment using
a non-UTC connection timezone may have stored local wall times; audit/repair
those values before this migration. Do not infer that the migration can discover
their original timezone. Production application rollback must retain these
historical tables; `down` is exercised only on disposable test schemas.

To run the PostgreSQL gates, point `MEGA_SNAPSHOT_TEST_DATABASE_URL` at an explicit
loopback-only disposable database named `snapshot_test`, never a production DB:

```bash
cargo test -p jupiter-migrate --lib snapshot --locked -- --include-ignored --nocapture
cargo test -p jupiter --lib snapshot_storage --locked
cargo test -p jupiter --lib namespace_storage --locked
cargo test -p ceres --lib snapshot --locked -- --include-ignored --nocapture
```

The ignored PostgreSQL tests fail without the URL, require loopback and the exact
test database name, and create independent schemas rather than refreshing public
tables. They retain those schemas for diagnosis. Verified locally on PostgreSQL
16.15: fresh migrations, concurrent source/node insertion, long scope paths,
transaction rollback, reconnect/readback and immutable radix roots. A separate
upgrade test verifies known UTC legacy values through forward/down/up migration
under an America/Los_Angeles session. The focused CI job also runs the ignored
million-binding index test. These gates do not validate publication, leases, GC,
writer fencing or the entire workspace; those remain separate acceptance work.

## Library API reference

```rust
use jupiter_migrate::{apply_migrations, Migrator};
```

`apply_migrations(&db, refresh)` runs pending migrations. `Migrator` is the SeaORM migrator trait implementation.
