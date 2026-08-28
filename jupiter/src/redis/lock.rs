use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use common::errors::MegaError;
use redis::{Script, aio::ConnectionManager};
use tokio::{
    sync::Notify,
    time::{Duration, sleep},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct RedLock {
    connection: ConnectionManager,
    key: String,
    value: String,
    ttl_ms: u64,
    stop_notify: Arc<Notify>,
}

impl RedLock {
    pub fn new(connection: ConnectionManager, key: impl Into<String>, ttl_ms: u64) -> Self {
        Self {
            connection,
            key: key.into(),
            value: Uuid::new_v4().to_string(),
            ttl_ms,
            stop_notify: Arc::new(Notify::new()),
        }
    }

    /// Try lock: SET key value NX PX TTL
    pub async fn try_lock(&self) -> Result<bool, MegaError> {
        let mut conn = self.connection.clone();
        // SET returns "OK" or Nil
        let result: Option<String> = redis::cmd("SET")
            .arg(&self.key)
            .arg(&self.value)
            .arg("NX")
            .arg("PX")
            .arg(self.ttl_ms)
            .query_async(&mut conn)
            .await?;

        Ok(result.is_some())
    }

    /// Retry interval while waiting for SET NX. A 200ms sleep turned a ~25ms
    /// hold into 200/400/600ms waits for anyone who missed the unlock.
    const LOCK_RETRY_SLEEP: Duration = Duration::from_millis(10);

    /// Lock with retry
    pub async fn lock(self: Arc<Self>) -> Result<RedLockGuard, MegaError> {
        let t0 = Instant::now();
        while !self.try_lock().await? {
            sleep(Self::LOCK_RETRY_SLEEP).await;
        }

        self.spawn_auto_renew();

        tracing::info!(
            lock_key = %self.key,
            ttl_ms = self.ttl_ms,
            waited_ms = t0.elapsed().as_millis(),
            "redlock acquired"
        );
        Ok(RedLockGuard {
            mutex: self,
            released: AtomicBool::new(false),
            acquired_at: t0,
        })
    }

    /// Unlock via Lua atomic script
    pub async fn unlock(&self) -> Result<bool, MegaError> {
        // stop renewer task
        self.stop_notify.notify_waiters();

        let script = Script::new(
            r#"
                if redis.call("GET", KEYS[1]) == ARGV[1] then
                    return redis.call("DEL", KEYS[1])
                else
                    return 0
                end
            "#,
        );

        let mut conn = self.connection.clone();
        let deleted: i32 = script
            .key(&self.key)
            .arg(&self.value)
            .invoke_async::<_>(&mut conn)
            .await?;

        Ok(deleted == 1)
    }

    /// Spawn TTL renew task
    fn spawn_auto_renew(self: &Arc<Self>) {
        let mutex = Arc::clone(self);

        let mut conn = self.connection.clone();
        tokio::spawn(async move {
            let half = mutex.ttl_ms / 2;

            loop {
                tokio::select! {
                    _ = sleep(Duration::from_millis(half)) => {},
                    _ = mutex.stop_notify.notified() => break,
                }

                // PEXPIRE returns integer reply
                let _: () = redis::cmd("PEXPIRE")
                    .arg(&mutex.key)
                    .arg(mutex.ttl_ms)
                    .query_async(&mut conn)
                    .await
                    .unwrap_or(());
            }
        });
    }
}

pub struct RedLockGuard {
    mutex: Arc<RedLock>,
    /// Tracks whether the lock has been explicitly released to prevent double-unlock
    released: AtomicBool,
    acquired_at: Instant,
}

impl RedLockGuard {
    pub async fn unlock(self) -> Result<(), MegaError> {
        // If already released, return immediately
        if self.released.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        tracing::info!(
            lock_key = %self.mutex.key,
            held_ms = self.acquired_at.elapsed().as_millis(),
            "redlock unlocking"
        );
        self.mutex.unlock().await?;
        Ok(())
    }
}

impl Drop for RedLockGuard {
    fn drop(&mut self) {
        // Cannot await in Drop, so spawn an async task
        let mutex = self.mutex.clone();
        let acquired_at = self.acquired_at;

        // Only unlock if it hasn't been released already (atomically set the flag)
        if !self.released.swap(true, Ordering::SeqCst) {
            tokio::spawn(async move {
                tracing::info!(
                    lock_key = %mutex.key,
                    held_ms = acquired_at.elapsed().as_millis(),
                    "redlock dropping guard; unlocking"
                );
                let _ = mutex.unlock().await;
            });
        }
    }
}

#[cfg(test)]
mod test {

    use std::{process::Command, sync::Arc};

    use futures::future::join_all;
    use redis::{AsyncCommands, aio::ConnectionManager};
    use redis_test::server::RedisServer;
    use tokio::time::{Duration, sleep, timeout};

    use crate::redis::lock::RedLock;

    fn redis_server_available() -> bool {
        Command::new("redis-server")
            .arg("--version")
            .output()
            .is_ok()
    }

    async fn init_server() -> Option<(RedisServer, ConnectionManager)> {
        if !redis_server_available() {
            eprintln!("redis-server not found; skipping redis lock tests");
            return None;
        }
        let server = RedisServer::new();
        let url = server.client_addr().to_owned();
        println!("starting redis mock server at: {}", url);
        let client = redis::Client::open(url).unwrap();
        let conn = ConnectionManager::new(client).await.unwrap();
        Some((server, conn))
    }

    #[tokio::test]
    async fn test_basic() {
        let Some((_server, mut conn)) = init_server().await else {
            return;
        };
        conn.set::<&str, _, String>("foo", "bar").await.unwrap();
        let v: String = conn.get("foo").await.unwrap();
        assert_eq!(v, "bar");
    }

    #[tokio::test]
    async fn test_try_lock() {
        let Some((_server, conn)) = init_server().await else {
            return;
        };
        let lock = Arc::new(RedLock::new(conn, "try_lock".to_string(), 3000));
        assert!(lock.try_lock().await.unwrap());
        assert!(!lock.try_lock().await.unwrap());
    }

    #[tokio::test]
    async fn test_unlock_script() {
        let Some((_server, conn)) = init_server().await else {
            return;
        };
        let lock = Arc::new(RedLock::new(conn, "unlock_script".to_string(), 3000));
        lock.try_lock().await.unwrap();
        assert!(lock.unlock().await.unwrap());
    }

    #[tokio::test]
    async fn test_auto_renew() {
        let Some((_server, mut conn)) = init_server().await else {
            return;
        };

        let lock = Arc::new(RedLock::new(conn.clone(), "renew".to_string(), 1000));

        let _guard = lock.clone().lock().await.unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;

        let ttl: i64 = redis::cmd("PTTL")
            .arg("renew")
            .query_async(&mut conn)
            .await
            .unwrap();

        assert!(ttl > 0);
    }

    #[tokio::test]
    async fn test_concurrent_locks() {
        let Some((_server, conn)) = init_server().await else {
            return;
        };

        let mut tasks = vec![];

        for _ in 0..64 {
            let lock = Arc::new(RedLock::new(conn.clone(), "race".to_string(), 3000));
            tasks.push(tokio::spawn(async move { lock.try_lock().await.unwrap() }));
        }

        let results = futures::future::join_all(tasks).await;

        let success_count = results
            .into_iter()
            .filter(|r| r.as_ref().unwrap() == &true)
            .count();

        assert_eq!(success_count, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_multi_pod_lock_sequential_acquisition() {
        let Some((_server, conn)) = init_server().await else {
            return;
        };

        let mut tasks = vec![];

        for pod_id in 0..3 {
            let conn = conn.clone();
            tasks.push(tokio::spawn(async move {
                let lock = Arc::new(RedLock::new(conn, "test-lock".to_string(), 1000));
                for i in 0..2 {
                    let guard = lock.clone().lock().await.unwrap();

                    println!("[pod-{pod_id}] acquired lock {i}");

                    // Sleep for longer than the TTL (1000ms) to test auto-renewal, but keep test fast.
                    sleep(Duration::from_millis(1200)).await;

                    guard.unlock().await.unwrap();
                }

                pod_id
            }));
        }

        let result = join_all(tasks).await;

        for r in result {
            assert!(r.is_ok());
        }
    }

    #[tokio::test]
    async fn test_single_pod_lock_starvation() {
        let Some((_server, conn)) = init_server().await else {
            return;
        };
        let lock = Arc::new(RedLock::new(conn, "test_lock", 1000));
        let guard = lock.clone().lock().await.unwrap();
        drop(guard);

        // Give the background unlock task time to complete to avoid race condition.
        sleep(Duration::from_millis(500)).await;

        let result = timeout(Duration::from_secs(2), lock.clone().lock()).await;
        match result {
            Ok(_) => println!("acquire lock successful"),
            Err(_) => panic!("acquire lock timeout, drop guard didn't unlock"),
        }
    }
}
