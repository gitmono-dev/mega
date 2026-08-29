use std::time::Duration;

use redis::{Script, aio::ConnectionManager};
use tokio::time::sleep;

use crate::utils::id_generator::{self, MAX_WORKER_ID};

const SLOT_KEY_PREFIX: &str = "mega:snowflake:worker:";
const SLOT_TTL_MS: u64 = 30_000;

/// Try to exclusive-claim a snowflake worker slot in Redis (`SET NX PX`).
///
/// Returns `None` if Redis errors or every slot in `0..=MAX_WORKER_ID` is taken.
/// On success, spawns a background PEXPIRE refresh so a live process keeps the slot.
pub async fn claim_snowflake_worker(connection: &ConnectionManager) -> Option<u32> {
    let identity = id_generator::process_identity();
    let mut conn = connection.clone();
    for id in 0..=MAX_WORKER_ID {
        let key = format!("{SLOT_KEY_PREFIX}{id}");
        let result: Result<Option<String>, _> = redis::cmd("SET")
            .arg(&key)
            .arg(&identity)
            .arg("NX")
            .arg("PX")
            .arg(SLOT_TTL_MS)
            .query_async(&mut conn)
            .await;
        match result {
            Ok(Some(_)) => {
                tracing::info!(
                    worker_id = id,
                    slot_key = %key,
                    identity = %identity,
                    "claimed snowflake worker slot"
                );
                spawn_slot_refresh(connection.clone(), key, identity);
                return Some(id);
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "snowflake worker slot claim failed; falling back to hash"
                );
                return None;
            }
        }
    }
    tracing::warn!(
        max = MAX_WORKER_ID,
        "all snowflake worker slots taken; falling back to hash"
    );
    None
}

fn spawn_slot_refresh(connection: ConnectionManager, key: String, value: String) {
    tokio::spawn(async move {
        let half = SLOT_TTL_MS / 2;
        let mut conn = connection;
        let script = Script::new(
            r#"
                if redis.call("GET", KEYS[1]) == ARGV[1] then
                    return redis.call("PEXPIRE", KEYS[1], ARGV[2])
                else
                    return 0
                end
            "#,
        );
        loop {
            sleep(Duration::from_millis(half)).await;
            let ok: Result<i32, _> = script
                .key(&key)
                .arg(&value)
                .arg(SLOT_TTL_MS)
                .invoke_async(&mut conn)
                .await;
            match ok {
                Ok(1) => {}
                Ok(_) => {
                    tracing::warn!(key = %key, "snowflake worker slot refresh lost");
                    break;
                }
                Err(e) => {
                    tracing::warn!(key = %key, error = %e, "snowflake worker slot refresh failed");
                    break;
                }
            }
        }
    });
}
