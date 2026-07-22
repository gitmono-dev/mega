# jupiter-migrate

SeaORM database migrations for Mega, extracted from `jupiter` so day-to-day `cargo check` does not compile migration code unless needed.

## Apply migrations

`mono` enables `jupiter/migrate`. On startup, `Storage::new` calls `jupiter_migrate::apply_migrations` automatically (`jupiter/src/storage/init.rs`). No separate `init` CLI step is required.

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

## Library API

```rust
use jupiter_migrate::{apply_migrations, Migrator};
```

`apply_migrations(&db, refresh)` runs pending migrations. `Migrator` is the SeaORM migrator trait implementation.
