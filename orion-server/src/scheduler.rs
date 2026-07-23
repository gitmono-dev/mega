use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use api_model::buck2::{
    status::Status,
    types::{ProjectRelativePath, TaskPhase},
    ws::WSMessage,
};
use chrono::FixedOffset;
use dashmap::DashMap;
use rand::RngExt;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter as _, prelude::DateTimeUtc,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify, mpsc::UnboundedSender};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    log::log_service::LogService,
    model::{dto::CoreWorkerStatus, internal::BuildTargetStateDTO, target_state::TargetState},
    repository::{
        build_events_repo::BuildEventsRepo, build_targets_repo::BuildTargetsRepo,
        orion_tasks_repo::OrionTasksRepo, target_state_histories_repo::TargetStateHistoriesRepo,
    },
};

/// Task queue configuration
#[derive(Debug, Clone)]
pub struct TaskQueueConfig {
    /// Maximum queue length
    pub max_queue_size: usize,
    /// Maximum wait time for tasks in queue
    pub max_wait_time: Duration,
    /// Queue cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for TaskQueueConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            max_wait_time: Duration::from_secs(30), // Fail builds with no worker after 30s
            cleanup_interval: Duration::from_secs(10), // Scan for expired builds every 10 seconds
        }
    }
}

/// Simple FIFO task queue
#[derive(Debug)]
pub struct TaskQueue {
    /// Queue storage for v2 (should replace the original queue in the future)
    queue_v2: VecDeque<PendingBuildEventV2>,
    /// Queue configuration
    config: TaskQueueConfig,
}

impl TaskQueue {
    pub fn new(config: TaskQueueConfig) -> Self {
        Self {
            queue_v2: VecDeque::new(),
            config,
        }
    }

    pub fn enqueue_v2(&mut self, task: PendingBuildEventV2) -> Result<(), String> {
        // Check if queue is full
        if self.queue_v2.len() >= self.config.max_queue_size {
            return Err("Queue is full".to_string());
        }

        self.queue_v2.push_back(task);
        Ok(())
    }

    /// Clean up expired queued builds (v2)
    pub fn cleanup_expired_v2(&mut self) -> Vec<PendingBuildEventV2> {
        let now = Instant::now();
        let mut expired_tasks = Vec::new();

        self.queue_v2.retain(|task| {
            if now.duration_since(task.created_at) > self.config.max_wait_time {
                expired_tasks.push(task.clone());
                false
            } else {
                true
            }
        });

        expired_tasks
    }

    /// Get queue statistics
    pub fn get_stats(&self) -> TaskQueueStats {
        TaskQueueStats {
            total_queued: self.queue_v2.len(),
            oldest_task_age_seconds: self
                .queue_v2
                .front()
                .map(|task| Instant::now().duration_since(task.created_at).as_secs()),
        }
    }
}

/// Queue statistics
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TaskQueueStats {
    pub total_queued: usize,
    /// Age of oldest task in seconds
    pub oldest_task_age_seconds: Option<u64>,
}

/// Mandatory Information for building a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildEventPayload {
    pub build_event_id: Uuid,
    pub task_id: Uuid,
    pub cl_link: String,
    pub repo: String,
    pub retry_count: i32,
}

#[derive(Debug, Clone)]
pub struct PendingBuildEventV2 {
    pub event_payload: BuildEventPayload,
    pub(crate) targets: Vec<BuildTargetStateDTO>,
    pub(crate) changes: Vec<Status<ProjectRelativePath>>,
    pub(crate) created_at: Instant,
}

/// Information for an active model
#[derive(Clone)]
pub struct BuildInfo {
    pub event_payload: BuildEventPayload,
    pub target_id: Uuid,
    pub target_path: String,
    pub changes: Vec<Status<ProjectRelativePath>>,
    pub started_at: DateTimeUtc,
    pub worker_id: String,
}

impl BuildEventPayload {
    pub fn new(
        build_event_id: Uuid,
        task_id: Uuid,
        cl_link: String,
        repo: String,
        retry_count: i32,
    ) -> Self {
        Self {
            build_event_id,
            task_id,
            cl_link,
            repo,
            retry_count,
        }
    }
}

/// Status of a worker node
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub enum WorkerStatus {
    Idle,
    Busy {
        // Cont ains build ID when busy
        build_id: String,
        // Show task phase when needed
        phase: Option<TaskPhase>,
    },
    Error(String), // Contains fail message
    Lost,          // Heartbeat timeout
}

impl WorkerStatus {
    pub fn status_type(&self) -> CoreWorkerStatus {
        match self {
            WorkerStatus::Idle => CoreWorkerStatus::Idle,
            WorkerStatus::Busy { .. } => CoreWorkerStatus::Busy,
            WorkerStatus::Error(_) => CoreWorkerStatus::Error,
            WorkerStatus::Lost => CoreWorkerStatus::Lost,
        }
    }
}

/// Information about a connected worker
#[derive(Debug)]
pub struct WorkerInfo {
    pub sender: UnboundedSender<WSMessage>,
    pub status: WorkerStatus,
    pub last_heartbeat: DateTimeUtc,
    pub hostname: String,
    pub start_time: DateTimeUtc,
    pub orion_version: String,
}

/// Task scheduler - manages task queue and worker assignment
#[derive(Clone)]
pub struct TaskScheduler {
    /// Pending task queue
    pub pending_tasks: Arc<Mutex<TaskQueue>>,
    /// Event notifier for new tasks or available workers
    pub task_notifier: Arc<Notify>,
    /// Worker information
    pub workers: Arc<DashMap<String, WorkerInfo>>,
    /// Active build tasks
    pub active_builds: Arc<DashMap<String, BuildInfo>>,
    /// Database connection
    pub conn: DatabaseConnection,
    /// Log service, used to persist explanatory logs for timed-out builds
    pub log_service: LogService,
}

impl TaskScheduler {
    /// Create new task scheduler instance
    pub fn new(
        conn: DatabaseConnection,
        workers: Arc<DashMap<String, WorkerInfo>>,
        active_builds: Arc<DashMap<String, BuildInfo>>,
        queue_config: Option<TaskQueueConfig>,
        log_service: LogService,
    ) -> Self {
        let config = queue_config.unwrap_or_default();
        Self {
            pending_tasks: Arc::new(Mutex::new(TaskQueue::new(config))),
            task_notifier: Arc::new(Notify::new()),
            workers,
            active_builds,
            conn,
            log_service,
        }
    }

    // NOTE: old `targets` table has been migrated away in favor of `callisto::build_targets`.

    pub async fn enqueue_task_v2(
        &self,
        task_id: Uuid,
        cl_link: &str,
        repo: String,
        changes: Vec<Status<ProjectRelativePath>>,
        retry_count: i32,
    ) -> Result<Uuid, String> {
        let build_event_id = Uuid::now_v7();

        self.enqueue_task_with_build_id_v2(
            build_event_id,
            task_id,
            cl_link,
            repo,
            changes,
            retry_count,
        )
        .await?;

        Ok(build_event_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn enqueue_task_with_build_id_v2(
        &self,
        build_event_id: Uuid,
        task_id: Uuid,
        cl_link: &str,
        repo: String,
        changes: Vec<Status<ProjectRelativePath>>,
        retry_count: i32,
    ) -> Result<(), String> {
        // Ensure the build event row exists in DB for queued tasks.
        BuildEventsRepo::insert_build(&self.conn, build_event_id, task_id, repo.clone())
            .await
            .map_err(|e| format!("Failed to insert build event: {e}"))?;

        // Ensure there is at least one default build target.
        let default_target_id = Uuid::now_v7();
        let _default_path =
            BuildTargetsRepo::insert_default_target(default_target_id, task_id, &self.conn)
                .await
                .map_err(|e| format!("Failed to insert default build target: {e}"))?;

        // Initialize a per-build target history record so UI can show "Pending" immediately.
        let now = chrono::Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap());
        let _ = callisto::target_state_histories::ActiveModel {
            id: Set(Uuid::now_v7()),
            build_target_id: Set(default_target_id),
            build_event_id: Set(build_event_id),
            target_state: Set(TargetState::Pending.to_string()),
            created_at: Set(now),
        }
        .insert(&self.conn)
        .await;

        let targets =
            BuildTargetsRepo::find_initialized_build_targets(build_event_id, task_id, &self.conn)
                .await
                .map_err(|e| format!("Failed to find initialized build targets: {e}"))?;
        let event = BuildEventPayload::new(
            build_event_id,
            task_id,
            cl_link.to_string(),
            repo,
            retry_count,
        );

        let pending_build_event = PendingBuildEventV2 {
            event_payload: event,
            targets,
            changes,
            created_at: Instant::now(),
        };

        {
            let mut queue = self.pending_tasks.lock().await;
            queue.enqueue_v2(pending_build_event)?;
        }

        // Notify that there's a new task to process
        self.task_notifier.notify_one();
        Ok(())
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> TaskQueueStats {
        let queue = self.pending_tasks.lock().await;
        queue.get_stats()
    }

    /// Clean up expired task-bound builds
    pub async fn cleanup_expired_tasks(&self) -> Vec<PendingBuildEventV2> {
        let mut queue = self.pending_tasks.lock().await;
        queue.cleanup_expired_v2()
    }

    async fn fail_expired_build_event(
        &self,
        build_event_id: Uuid,
        task_id: Uuid,
        repo: &str,
        wait_secs: u64,
    ) {
        let message = format!(
            "No build worker became available within {wait_secs}s; build marked as Interrupted.\n"
        );
        self.finalize_interrupted_build(build_event_id, task_id, repo, &message)
            .await;
    }

    /// Immediately finalize a build that cannot be scheduled because no worker is
    /// available, marking it `Interrupted` with an explanatory log. Used instead
    /// of queuing the build when there are no idle workers, so the UI shows a
    /// clear terminal state (retryable) rather than a build stuck in the queue.
    pub async fn fail_unschedulable_build(&self, build_event_id: Uuid, task_id: Uuid, repo: &str) {
        self.finalize_interrupted_build(
            build_event_id,
            task_id,
            repo,
            "No build worker is currently available; build marked as Interrupted.\n",
        )
        .await;
    }

    /// Core, idempotent build finalization: atomically claim the interrupt
    /// transition, then (only if we won the claim) persist an explanatory log
    /// line, mark the local log complete, upload it, and move the task's targets
    /// to `Interrupted`.
    async fn finalize_interrupted_build(
        &self,
        build_event_id: Uuid,
        task_id: Uuid,
        repo: &str,
        message: &str,
    ) {
        let now = chrono::Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap());

        // Atomically claim the interrupt transition first. If another path (the
        // in-memory timeout, the DB stale-build sweep, or startup reconciliation)
        // already finalized this build, `rows_affected` is 0 and we must not write
        // a second explanatory log line or duplicate target history.
        match BuildEventsRepo::mark_interrupted(build_event_id, now, &self.conn).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                tracing::error!(
                    "Failed to mark build {} as interrupted: {}",
                    build_event_id,
                    e
                );
                return;
            }
        }

        // We won the claim: persist an explanatory log line so the build does not
        // appear as a silent, empty failure in the UI, mark the local log
        // complete, and upload it to cloud, mirroring the normal completion flow.
        let repo_segment = LogService::last_segment(repo);
        let task_id_str = task_id.to_string();
        let build_id_str = build_event_id.to_string();
        if let Err(e) = self
            .log_service
            .append_local(&task_id_str, &repo_segment, &build_id_str, message)
            .await
        {
            tracing::error!(
                "Failed to write interrupt log for build {}: {}",
                build_event_id,
                e
            );
        }
        self.log_service
            .mark_local_complete(&task_id_str, &repo_segment, &build_id_str)
            .await;
        self.log_service.spawn_cloud_upload(
            task_id_str.clone(),
            repo_segment,
            build_id_str.clone(),
        );

        match BuildTargetsRepo::list_by_task_id(&self.conn, task_id).await {
            Ok(targets) => {
                for target in targets {
                    // Never overwrite a target that already reached a final state.
                    if TargetState::from(target.latest_state.clone()).is_terminal() {
                        continue;
                    }
                    if let Err(e) = BuildTargetsRepo::update_latest_state(
                        &self.conn,
                        target.id,
                        TargetState::Interrupted,
                    )
                    .await
                    {
                        tracing::error!(
                            "Failed to set target {} to Interrupted for expired build {}: {}",
                            target.id,
                            build_event_id,
                            e
                        );
                    }
                    if let Err(e) = TargetStateHistoriesRepo::upsert_state(
                        &self.conn,
                        target.id,
                        build_event_id,
                        TargetState::Interrupted.to_string(),
                        now,
                    )
                    .await
                    {
                        tracing::error!(
                            "Failed to record Interrupted history for target {} build {}: {}",
                            target.id,
                            build_event_id,
                            e
                        );
                    }
                }
            }
            Err(e) => tracing::error!("Failed to list targets for expired task {}: {}", task_id, e),
        }
    }

    /// Mark a build that timed out in the in-memory queue (no worker became
    /// available) as ended.
    async fn fail_expired_build(&self, expired: &PendingBuildEventV2) {
        let wait_secs = {
            let queue = self.pending_tasks.lock().await;
            queue.config.max_wait_time.as_secs()
        };

        self.fail_expired_build_event(
            expired.event_payload.build_event_id,
            expired.event_payload.task_id,
            &expired.event_payload.repo,
            wait_secs,
        )
        .await;
    }

    /// DB-level safety net for queued builds that are no longer present in this
    /// process's in-memory queue (for example after restart or multi-instance
    /// routing). These would otherwise remain `Uninitialized` forever.
    async fn cleanup_stale_queued_builds(&self) {
        let max_wait_time = {
            let queue = self.pending_tasks.lock().await;
            queue.config.max_wait_time
        };
        let wait_secs = max_wait_time.as_secs();
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::from_std(max_wait_time)
                .unwrap_or_else(|_| chrono::Duration::seconds(30)))
        .with_timezone(&FixedOffset::east_opt(0).unwrap());

        let stale_builds = match BuildEventsRepo::list_unfinished_before(&self.conn, cutoff).await {
            Ok(builds) => builds,
            Err(e) => {
                tracing::error!("Failed to list stale queued builds: {}", e);
                return;
            }
        };

        for build in stale_builds {
            let build_id = build.id.to_string();
            if self.active_builds.contains_key(&build_id) {
                continue;
            }

            let targets = match BuildTargetsRepo::list_by_task_id(&self.conn, build.task_id).await {
                Ok(targets) => targets,
                Err(e) => {
                    tracing::error!(
                        "Failed to list targets while checking stale build {}: {}",
                        build.id,
                        e
                    );
                    continue;
                }
            };

            let is_still_queued = targets
                .iter()
                .any(|target| target.latest_state == TargetState::Uninitialized.to_string());
            if !is_still_queued {
                continue;
            }

            let repo = match OrionTasksRepo::find_by_id(&self.conn, build.task_id).await {
                Ok(Some(task)) => task.repo_name,
                Ok(None) => {
                    tracing::warn!(
                        "Skipping stale build {} because task {} no longer exists",
                        build.id,
                        build.task_id
                    );
                    continue;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to load task {} for stale build {}: {}",
                        build.task_id,
                        build.id,
                        e
                    );
                    continue;
                }
            };

            tracing::warn!(
                "DB stale queued build {}/{} exceeded wait window; marking as interrupted",
                build.task_id,
                build.id
            );
            self.fail_expired_build_event(build.id, build.task_id, &repo, wait_secs)
                .await;
        }
    }

    /// One-shot reconciliation run at process startup.
    ///
    /// A freshly started process has an empty `active_builds`, so any build still
    /// marked unfinished (`end_at IS NULL`) is an orphan left behind by a previous
    /// instance that crashed or restarted mid-build (e.g. a build stuck in
    /// `Building`). Without this, such builds — and their `Building`/`Pending`
    /// targets — would never reach a terminal state, since the periodic queue
    /// sweep only reconciles `Uninitialized` (still-queued) builds and the
    /// dead-worker check only covers currently-registered workers.
    pub async fn reconcile_orphaned_builds_on_startup(&self) {
        let now = chrono::Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap());

        let orphaned = match BuildEventsRepo::list_unfinished_before(&self.conn, now).await {
            Ok(builds) => builds,
            Err(e) => {
                tracing::error!(
                    "Startup reconciliation failed to list unfinished builds: {}",
                    e
                );
                return;
            }
        };

        if orphaned.is_empty() {
            return;
        }

        tracing::warn!(
            "Startup reconciliation: finalizing {} orphaned unfinished build(s)",
            orphaned.len()
        );

        for build in orphaned {
            // Defensive: never touch a build this process is actively running.
            if self.active_builds.contains_key(&build.id.to_string()) {
                continue;
            }

            let repo = match OrionTasksRepo::find_by_id(&self.conn, build.task_id).await {
                Ok(Some(task)) => task.repo_name,
                Ok(None) => {
                    tracing::warn!(
                        "Startup reconciliation skipping build {} because task {} no longer exists",
                        build.id,
                        build.task_id
                    );
                    continue;
                }
                Err(e) => {
                    tracing::error!(
                        "Startup reconciliation failed to load task {} for build {}: {}",
                        build.task_id,
                        build.id,
                        e
                    );
                    continue;
                }
            };

            tracing::warn!(
                "Startup reconciliation: marking orphaned build {}/{} as interrupted",
                build.task_id,
                build.id
            );
            self.finalize_interrupted_build(
                build.id,
                build.task_id,
                &repo,
                "Build server restarted while this build was in progress; marked as Interrupted.\n",
            )
            .await;
        }
    }

    /// Check if there are available workers
    pub fn has_idle_workers(&self) -> bool {
        self.workers
            .iter()
            .any(|entry| matches!(entry.value().status, WorkerStatus::Idle))
    }

    /// Get list of idle workers
    pub fn get_idle_workers(&self) -> Vec<String> {
        self.workers
            .iter()
            .filter(|entry| matches!(entry.value().status, WorkerStatus::Idle))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Search available worker and claim the worker for current build
    pub fn search_and_claim_worker(&self, build_id: &str) -> Option<String> {
        let idle_workers: Vec<String> = self
            .workers
            .iter()
            .filter(|entry| matches!(entry.value().status, WorkerStatus::Idle))
            .map(|entry| entry.key().clone())
            .collect();
        let chosen_worker_idx = {
            let mut rng = rand::rng();
            rng.random_range(0..idle_workers.len())
        };
        let chosen_worker_id = idle_workers[chosen_worker_idx].clone();
        if let Some(mut worker) = self.workers.get_mut(&chosen_worker_id) {
            worker.status = WorkerStatus::Busy {
                build_id: build_id.to_string(),
                phase: None,
            };
            Some(chosen_worker_id)
        } else {
            None
        }
    }

    pub async fn release_worker(&self, worker_id: &str) {
        tracing::info!("Releasing worker {} back to idle", worker_id);
        if let Some(mut worker) = self.workers.get_mut(worker_id) {
            worker.status = WorkerStatus::Idle;
        }
    }

    /// Register `build_info` on `chosen_id` and send `TaskBuild`.
    ///
    /// The `workers` DashMap write guard is dropped before this returns. Callers
    /// must not hold that guard across `.await` — doing so can stall the entire
    /// tokio runtime when heartbeats / health checks / clients-info contend on
    /// the same shard (seen as HTTP hang on `/mega/oc` while the process is up).
    pub fn claim_worker_for_build(
        &self,
        chosen_id: &str,
        build_info: BuildInfo,
        msg: WSMessage,
    ) -> Result<(), String> {
        claim_worker_for_build(
            &self.workers,
            &self.active_builds,
            chosen_id,
            build_info,
            msg,
        )
    }

    /// Try to dispatch queued task-bound builds (concurrent safe)
    pub async fn process_pending_tasks(&self) {
        // Get available workers
        let idle_workers = self.get_idle_workers();
        if idle_workers.is_empty() {
            return;
        }

        // Process tasks in batches, up to the number of idle workers
        let max_tasks = idle_workers.len();
        let mut tasks_to_dispatch = Vec::with_capacity(max_tasks);

        // Batch dequeue tasks
        {
            let mut queue = self.pending_tasks.lock().await;
            for _ in 0..max_tasks {
                if let Some(task) = queue.queue_v2.pop_front() {
                    tasks_to_dispatch.push(task);
                } else {
                    break;
                }
            }
        }

        // Dispatch tasks concurrently
        if !tasks_to_dispatch.is_empty() {
            let dispatch_futures: Vec<_> = tasks_to_dispatch
                .into_iter()
                .map(|task| {
                    let scheduler = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = scheduler.dispatch_task_v2(task).await {
                            tracing::error!("Failed to dispatch queued task: {}", e);
                        }
                    })
                })
                .collect();

            // Wait for all dispatch tasks to complete
            for future in dispatch_futures {
                let _ = future.await;
            }
        }
    }

    /// Dispatch single queued v2 task
    async fn dispatch_task_v2(
        &self,
        pending_build_event: PendingBuildEventV2,
    ) -> Result<(), String> {
        let idle_workers = self.get_idle_workers();
        if idle_workers.is_empty() {
            return Err("No idle workers available".to_string());
        }

        // Randomly select an idle worker
        let chosen_index = {
            let mut rng = rand::rng();
            rng.random_range(0..idle_workers.len())
        };
        let chosen_id = idle_workers[chosen_index].clone();
        let start_at = chrono::Utc::now();
        let start_at_tz = start_at.with_timezone(&FixedOffset::east_opt(0).unwrap());

        // Choose a single default build target for this execution (current worker protocol builds one target).
        let target_id = pending_build_event
            .targets
            .first()
            .map(|t| t.id)
            .unwrap_or_else(Uuid::nil);
        let target_path = pending_build_event
            .targets
            .first()
            .map(|t| t.path.clone())
            .unwrap_or_else(|| "//".to_string());

        // Create build information (target_id is a build_target_id in the new schema)
        let build_info = BuildInfo {
            event_payload: pending_build_event.event_payload.clone(),
            changes: pending_build_event.changes.clone(),
            target_id,
            target_path: target_path.clone(),
            worker_id: chosen_id.clone(),
            started_at: start_at,
        };

        // Ensure a per-build target history row exists for this build/target.
        let _ = callisto::target_state_histories::ActiveModel {
            id: Set(Uuid::now_v7()),
            build_target_id: Set(target_id),
            build_event_id: Set(pending_build_event.event_payload.build_event_id),
            target_state: Set(TargetState::Building.to_string()),
            created_at: Set(start_at_tz),
        }
        .insert(&self.conn)
        .await;

        let _ = callisto::build_targets::Entity::update_many()
            .filter(callisto::build_targets::Column::Id.eq(target_id))
            .set(callisto::build_targets::ActiveModel {
                latest_state: Set(TargetState::Building.to_string()),
                ..Default::default()
            })
            .exec(&self.conn)
            .await;

        // Create WebSocket message
        let msg = WSMessage::TaskBuild {
            build_id: pending_build_event.event_payload.build_event_id.to_string(),
            repo: pending_build_event.event_payload.repo,
            cl_link: pending_build_event.event_payload.cl_link.to_string(),
            changes: pending_build_event.changes.clone(),
        };

        // Register the build *before* sending TaskBuild so early TaskBuildOutput lines
        // from the worker are not dropped (ws_handler only publishes logs when
        // active_builds contains the build_id). Guard is dropped inside the helper
        // before any further awaits in the caller.
        self.claim_worker_for_build(&chosen_id, build_info, msg)?;
        tracing::info!(
            "Queued task {}/{} dispatched to worker {}",
            pending_build_event.event_payload.task_id,
            pending_build_event.event_payload.build_event_id,
            chosen_id
        );
        Ok(())
    }

    /// Notify about new task or available worker
    pub fn notify_task_available(&self) {
        self.task_notifier.notify_one();
    }

    /// Start queue management background task (event-driven + periodic cleanup)
    pub async fn start_queue_manager(self) {
        let cleanup_interval = {
            let queue = self.pending_tasks.lock().await;
            queue.config.cleanup_interval
        };

        // Task dispatcher: wait for notifications or process periodically
        let dispatch_scheduler = self.clone();
        let dispatch_task = tokio::spawn(async move {
            loop {
                // Wait for notification or timeout
                tokio::select! {
                    // Wait for new task or worker available notification
                    _ = dispatch_scheduler.task_notifier.notified() => {
                        dispatch_scheduler.process_pending_tasks().await;
                    }
                    // Periodic check (prevent missing notifications)
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {
                        dispatch_scheduler.process_pending_tasks().await;
                    }
                }
            }
        });

        // Cleaner: periodically clean up expired tasks
        let cleanup_scheduler = self.clone();
        let cleanup_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);

            loop {
                interval.tick().await;

                // Clean up expired tasks
                let expired_tasks = cleanup_scheduler.cleanup_expired_tasks().await;
                if !expired_tasks.is_empty() {
                    tracing::warn!(
                        "Cleaned up {} expired tasks from queue",
                        expired_tasks.len()
                    );

                    // Mark each timed-out build as ended and its targets interrupted.
                    for task in expired_tasks {
                        tracing::warn!(
                            "No worker within wait window; marking build {}/{} ({}) as interrupted",
                            task.event_payload.task_id,
                            task.event_payload.build_event_id,
                            task.event_payload.repo
                        );
                        cleanup_scheduler.fail_expired_build(&task).await;
                    }
                }

                cleanup_scheduler.cleanup_stale_queued_builds().await;
            }
        });

        // Wait for tasks to complete (actually runs forever)
        tokio::select! {
            _ = dispatch_task => {
                tracing::error!("Task dispatcher unexpectedly stopped");
            }
            _ = cleanup_task => {
                tracing::error!("Task cleanup unexpectedly stopped");
            }
        }
    }
}

/// Register `build_info` on `chosen_id` and send `msg`.
///
/// Must drop the `workers` DashMap write guard before returning (no `.await`
/// while holding it). See [`TaskScheduler::claim_worker_for_build`].
pub(crate) fn claim_worker_for_build(
    workers: &DashMap<String, WorkerInfo>,
    active_builds: &DashMap<String, BuildInfo>,
    chosen_id: &str,
    build_info: BuildInfo,
    msg: WSMessage,
) -> Result<(), String> {
    let key = build_info.event_payload.build_event_id.to_string();
    let Some(mut worker) = workers.get_mut(chosen_id) else {
        return Err(format!("Worker {chosen_id} not found"));
    };
    active_builds.insert(key.clone(), build_info);
    worker.status = WorkerStatus::Busy {
        build_id: key.clone(),
        phase: None,
    };
    if worker.sender.send(msg).is_err() {
        active_builds.remove(&key);
        worker.status = WorkerStatus::Idle;
        return Err(format!("Failed to send task to worker {chosen_id}"));
    }
    Ok(())
    // RefMut dropped here — safe for callers to `.await` afterward.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_queue_fifo() {
        let config = TaskQueueConfig::default();
        let mut queue = TaskQueue::new(config);

        let build_event1 = BuildEventPayload::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            "test_cl_link".to_string(),
            "/test/repo".to_string(),
            0,
        );

        let task1 = PendingBuildEventV2 {
            event_payload: build_event1.clone(),
            targets: vec![],
            changes: vec![],
            created_at: Instant::now(),
        };

        let build_event2 = BuildEventPayload::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            "test_cl_link_2".to_string(),
            "/test2/repo".to_string(),
            0,
        );

        let task2 = PendingBuildEventV2 {
            event_payload: build_event2.clone(),
            targets: vec![],
            changes: vec![],
            created_at: Instant::now(),
        };

        assert!(queue.enqueue_v2(task1).is_ok());
        assert!(queue.enqueue_v2(task2).is_ok());

        // check stats
        let stats = queue.get_stats();
        assert_eq!(stats.total_queued, 2);
    }

    /// Test queue capacity limit
    #[test]
    fn test_queue_capacity() {
        let config = TaskQueueConfig {
            max_queue_size: 2,
            max_wait_time: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(30),
        };
        let mut queue = TaskQueue::new(config);

        let build_event = BuildEventPayload::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            "test_cl_link".to_string(),
            "test/repo".to_string(),
            0,
        );
        let task = PendingBuildEventV2 {
            event_payload: build_event.clone(),
            targets: vec![],
            changes: vec![],
            created_at: Instant::now(),
        };

        // Fill queue to capacity
        assert!(queue.enqueue_v2(task.clone()).is_ok());
        assert!(queue.enqueue_v2(task.clone()).is_ok());

        // Should fail when full
        assert!(queue.enqueue_v2(task).is_err());
    }

    /// Regression: after claim+send, the workers DashMap shard must be free so
    /// concurrent heartbeats / health checks / clients-info cannot block the
    /// runtime (the old retry path held `get_mut` across DB `.await`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claim_worker_for_build_releases_guard_before_return() {
        let workers = Arc::new(DashMap::new());
        let active_builds = Arc::new(DashMap::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let worker_id = "worker-1".to_string();
        workers.insert(
            worker_id.clone(),
            WorkerInfo {
                sender: tx,
                status: WorkerStatus::Idle,
                last_heartbeat: chrono::Utc::now(),
                start_time: chrono::Utc::now(),
                hostname: "test-host".to_string(),
                orion_version: "0.0.0".to_string(),
            },
        );

        let build_event_id = Uuid::now_v7();
        let task_id = Uuid::now_v7();
        let build_info = BuildInfo {
            event_payload: BuildEventPayload::new(
                build_event_id,
                task_id,
                "cl".to_string(),
                "/repo".to_string(),
                1,
            ),
            target_id: Uuid::now_v7(),
            target_path: "//".to_string(),
            changes: vec![],
            started_at: chrono::Utc::now(),
            worker_id: worker_id.clone(),
        };
        let msg = WSMessage::TaskBuild {
            build_id: build_event_id.to_string(),
            repo: "/repo".to_string(),
            cl_link: "cl".to_string(),
            changes: vec![],
        };

        claim_worker_for_build(&workers, &active_builds, &worker_id, build_info, msg)
            .expect("claim should succeed");

        // Simulate the DB awaits that follow claim on the retry path.
        let workers_for_contended = workers.clone();
        let contended = tokio::spawn(async move {
            // Would hang forever if claim still held the shard lock across an await.
            let guard = workers_for_contended
                .get_mut("worker-1")
                .expect("worker must remain reachable");
            matches!(
                guard.status,
                WorkerStatus::Busy {
                    build_id: _,
                    phase: None
                }
            )
        });

        let ok = tokio::time::timeout(Duration::from_millis(500), contended)
            .await
            .expect("workers map must stay accessible after claim returns")
            .expect("join");
        assert!(ok, "worker should be Busy after claim");
        assert!(active_builds.contains_key(&build_event_id.to_string()));
        assert!(matches!(
            rx.try_recv(),
            Ok(WSMessage::TaskBuild { build_id, .. }) if build_id == build_event_id.to_string()
        ));
    }
}
