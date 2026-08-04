//! Application-level data backfills run on mono boot (after schema migrations).

mod actor_identity;

pub use actor_identity::spawn_actor_identity_backfill;
