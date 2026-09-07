# Namespace publication transaction core

Status: implemented storage core, 2026-09-06; **not an enabled publisher API**.
The application composer, all production writer integrations, release-policy
enforcement, prepare/retention pins and authorization remain required before
the namespace capability can be announced.

## Ownership and ordering

`PublicationStorage::begin(request, expected_head, writer_epoch)` owns one
database transaction. Its first write reserves the unique
`(actor_domain, operation_id)` row. A duplicate committed request returns its
receipt before exposing any ref-writing handle. Reusing the same committed key
with a different request digest or instance is a conflict. Failed/aborted
transactions leave no operation reservation or success receipt.

The request digest is supplied by a trusted application adapter and MUST cover
the complete canonical mutation plan: fixed base/head, expected refs, binding
policy/read set and prepared content identities. The storage facade cannot
infer these fields from an opaque digest. The authenticated actor domain must
not be accepted from untrusted request JSON. Receipt reads require current
authorization independently of the operation key.

A ready result owns `PublicationTransaction`. Writers can borrow its underlying
transaction for conditional refs, prepared metadata, scope attestations and
index nodes, but cannot obtain ownership and independently commit it. Explicit
abort or dropping the owner rolls back. Publication's `finish` is the only
commit path exposed by this wrapper.

`finish` validates the prepared view identity/byte bound and same instance, then
stages insert-only view bytes, conditional head update, publication history,
operation result and outbox event. They commit together with the borrowed
transaction's ref changes. A database error after head CAS still rolls back
the head, view and refs. No notification is dispatched before COMMIT.

## Compare-and-swap and receipts

The head condition includes instance, expected sequence, expected view ID and
writer epoch. Bootstrap is an insert-if-absent head, not an upsert. Sequences and
epochs are positive SQL BIGINT values and sequence increment checks overflow.

When the descriptor is unchanged, the operation may be a no-op for namespace
publication: preserve sequence/view and do not insert publication/outbox rows.
It STILL executes the head/epoch fence. For example, a non-selected branch may
change without changing the default namespace view. Determining that the view
really represents the complete post-write state belongs to the application;
the storage facade must not be used to hide a selected-ref mutation.

`GitDbStorage::update_ref_if_unchanged` adds one conditional SQL update on
repo ID, fully qualified ref name and expected object ID, returning whether
exactly one row changed. It accepts the publication transaction and does not
silently rebase/retry. Existing legacy writers are not yet switched to this
method. The caller must abort the whole publication if any required ref/read
condition fails.

A COMMIT error is reported as an uncertain outcome, not a proven rollback.
Look up the original actor/operation/request digest on a new connection before
retrying. Receipt replay never dispatches a second ref mutation or outbox
event. Outbox rows have unique event IDs and pending/delivered state, but the
delivery worker and external side effects are not implemented by this core.

A writer_epoch column does not fence an old binary that never checks it.
Maintenance cutover and an audit of every production writer remain G04/G05
requirements; a passing storage test cannot establish those conditions.

## Schema and reproduction

The additive migration `m20260906_160000_namespace_publication` creates
namespace_view, namespace_head, namespace_publication, snapshot_operation and
namespace_outbox. It creates no initial head/catalog and enables no feature.
Generated Callisto fields were produced with sea-orm-cli 2.0.2 from the actual
SQLite migration schema; PostgreSQL tests verify the same runtime schema.

View payloads are bounded to 16 KiB in SQL and checked against their SHA-256 ID.
The application supplies the already validated namespace-manifest-v1 codec.
An opaque-byte storage fixture is not proof that the manifest describes the
actual native/import objects. No foreign-key cascade from a mutable ref or
registry path deletes published metadata. Retention/GC and referential audits
must be supplied by the full publisher before deployment.

Use the explicit loopback disposable PostgreSQL URL described in
[jupiter-migrate](../../jupiter-migrate/README.md), then run:

```bash
cargo test -p jupiter --lib publication_storage --locked -- --include-ignored --nocapture
cargo test -p jupiter-migrate --lib snapshot --locked -- --include-ignored --nocapture
```

The six publication tests cover SQLite lifecycle, duplicate-key concurrency and
expected-old competition, plus PostgreSQL lifecycle/reconnect, concurrent
duplicates/expected-old writers and an independent-connection epoch change.
Shared lifecycle checks also inject failure after head CAS, drop an uncommitted
transaction, reject different-payload replay, preserve old views and verify
no-op ref writes. They use the REAL import_refs and new publication tables.
PostgreSQL tests create fresh random schemas, retain diagnostics and never
refresh a supplied database. Tests do not cover a process/host power loss,
external payload durability, notification delivery, source/path permissions or
release-policy bypass through actual production routes.

The CI focused snapshot job runs these PostgreSQL tests explicitly instead of
silently skipping ignored tests. MG06/MG09/MG15 have additional storage-level
evidence; the broader acceptance IDs remain incomplete until application and
real-service integration are tested.
