use std::{
    collections::HashSet,
    convert::Infallible,
    path::{Component, Path},
    pin::Pin,
    time::Duration,
};

use api_model::{
    buck2::{
        api::{OrionBuildResult, OrionServerResponse, TaskBuildRequest},
        status::Status,
        types::{
            LogErrorResponse, LogLinesResponse, LogReadMode, ProjectRelativePath,
            TargetLogLinesResponse, TargetLogQuery, TaskHistoryQuery,
        },
        ws::WSMessage,
    },
    common::{CommonPage, PageParams},
};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
};
use futures::stream::select;
use futures_util::{Stream, StreamExt};
use rand::RngExt;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_stream::wrappers::IntervalStream;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    model::{
        dto::{
            BuildEventDTO, BuildStatus, BuildTargetDTO, MessageResponse, OrionClientInfo,
            OrionClientQuery, OrionClientStatus, OrionTaskDTO,
        },
        target_state::TargetState,
    },
    repository::{
        build_events_repo::BuildEventsRepo, build_targets_repo::BuildTargetsRepo,
        orion_tasks_repo::OrionTasksRepo, target_state_histories_repo::TargetStateHistoriesRepo,
    },
    scheduler::{BuildEventPayload, BuildInfo, TaskQueueStats, WorkerStatus},
};

type MessageErrorResponse = (StatusCode, Json<MessageResponse>);
type JsonValueErrorResponse = (StatusCode, Json<Value>);
type LogSseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

/// Normalize incoming task changes to the Orion worker contract.
///
/// Files inside the requested repo root should be repo-relative, while shared
/// files outside that root must remain monorepo-relative.
fn normalize_repo_root_changes(
    repo: &str,
    changes: Vec<Status<ProjectRelativePath>>,
) -> Vec<Status<ProjectRelativePath>> {
    // Avoid reserving memory directly from request-controlled input length.
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    let repo_prefix = repo.trim_matches('/');
    let repo_prefix_with_slash = (!repo_prefix.is_empty()).then(|| format!("{repo_prefix}/"));

    for change in changes {
        let normalized_change = match change {
            Status::Modified(path) => {
                normalize_change_path(path, repo_prefix, repo_prefix_with_slash.as_deref())
                    .map(Status::Modified)
            }
            Status::Added(path) => {
                normalize_change_path(path, repo_prefix, repo_prefix_with_slash.as_deref())
                    .map(Status::Added)
            }
            Status::Removed(path) => {
                normalize_change_path(path, repo_prefix, repo_prefix_with_slash.as_deref())
                    .map(Status::Removed)
            }
        };

        if let Some(normalized_change) = normalized_change
            && seen.insert(normalized_change.clone())
        {
            normalized.push(normalized_change);
        }
    }

    normalized
}

fn normalize_change_path(
    path: ProjectRelativePath,
    repo_prefix: &str,
    repo_prefix_with_slash: Option<&str>,
) -> Option<ProjectRelativePath> {
    let raw = path.as_str().trim_start_matches('/');
    let canonical = if repo_prefix.is_empty() {
        raw.to_string()
    } else if raw == repo_prefix {
        String::new()
    } else if let Some(prefix) = repo_prefix_with_slash {
        if let Some(stripped) = raw.strip_prefix(prefix) {
            stripped.to_string()
        } else {
            raw.to_string()
        }
    } else {
        raw.to_string()
    };

    if !is_safe_normalized_path(&canonical) {
        tracing::warn!(
            path = %canonical,
            "Dropping unsafe task change path after normalization."
        );
        return None;
    }

    Some(ProjectRelativePath::new(&canonical))
}

fn is_safe_normalized_path(path: &str) -> bool {
    path.is_empty()
        || (!path.contains("//")
            && Path::new(path)
                .components()
                .all(|component| matches!(component, Component::Normal(_))))
}

pub async fn task_output(state: &AppState, id: &str) -> Result<Sse<LogSseStream>, StatusCode> {
    if !state.scheduler.active_builds.contains_key(id) {
        return Err(StatusCode::NOT_FOUND);
    }

    let (stop_tx, stop_rx) = watch::channel(true);

    let log_stop_rx = stop_rx.clone();
    let build_id = id.to_string();
    let log_stream = state
        .log_service
        .subscribe_for_build(build_id.clone())
        .map(|log_event| {
            Ok::<Event, Infallible>(Event::default().event("log").data(log_event.line))
        })
        .take_while(move |_| {
            let stop_rx = log_stop_rx.clone();
            async move { *stop_rx.borrow() }
        });

    let heart_stop_rx = stop_rx.clone();
    let heartbeat_stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(15)))
        .map(|_| Ok::<Event, Infallible>(Event::default().comment("heartbeat")))
        .take_while(move |_| {
            let stop_rx_clone = heart_stop_rx.clone();
            async move { *stop_rx_clone.borrow() }
        });

    let stop_tx_clone = stop_tx.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !state_clone.scheduler.active_builds.contains_key(&build_id) {
                let _ = stop_tx_clone.send(false);
                break;
            }
        }
    });

    Ok(Sse::new(Box::pin(select(log_stream, heartbeat_stream))))
}

fn message_response(message: impl Into<String>) -> Json<MessageResponse> {
    Json(MessageResponse {
        message: message.into(),
    })
}

fn message_error(status: StatusCode, message: impl Into<String>) -> MessageErrorResponse {
    (status, message_response(message))
}

fn value_error(status: StatusCode, message: impl Into<String>) -> JsonValueErrorResponse {
    (status, Json(json!({ "message": message.into() })))
}

fn parse_uuid_or_message_error(
    raw_id: &str,
    invalid_message: &str,
) -> Result<Uuid, MessageErrorResponse> {
    raw_id
        .parse::<Uuid>()
        .map_err(|_| message_error(StatusCode::BAD_REQUEST, invalid_message))
}

fn parse_uuid_or_value_error(
    raw_id: &str,
    invalid_message: &str,
) -> Result<Uuid, JsonValueErrorResponse> {
    raw_id
        .parse::<Uuid>()
        .map_err(|_| value_error(StatusCode::BAD_REQUEST, invalid_message))
}

async fn task_exists_by_id(
    conn: &sea_orm::DatabaseConnection,
    task_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    OrionTasksRepo::exists_by_id(conn, task_id).await
}

pub async fn task_get(
    state: &AppState,
    cl: &str,
) -> Result<Json<Vec<OrionTaskDTO>>, JsonValueErrorResponse> {
    let tasks = OrionTasksRepo::find_by_cl(&state.conn, cl)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch tasks by CL {}: {}", cl, e);
            value_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    Ok(Json(tasks.iter().map(OrionTaskDTO::from).collect()))
}

pub async fn build_event_get(
    state: &AppState,
    task_id: &str,
) -> Result<Json<Vec<BuildEventDTO>>, JsonValueErrorResponse> {
    let task_uuid = parse_uuid_or_value_error(task_id, "Invalid task ID")?;
    let task_exists = task_exists_by_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify task existence {}: {}", task_id, e);
            value_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;
    if !task_exists {
        return Err(value_error(StatusCode::NOT_FOUND, "Task not found"));
    }

    let build_events = BuildEventsRepo::list_by_task_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch build events for task {}: {}", task_id, e);
            value_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    Ok(Json(
        build_events
            .into_iter()
            .map(|m| BuildEventDTO::from(&m))
            .collect(),
    ))
}

pub async fn targets_get(
    state: &AppState,
    task_id: &str,
) -> Result<Json<Vec<BuildTargetDTO>>, MessageErrorResponse> {
    let task_uuid = parse_uuid_or_message_error(task_id, "Invalid task ID")?;
    let task_exists = task_exists_by_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify task existence {}: {}", task_id, e);
            message_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;
    if !task_exists {
        return Err(message_error(StatusCode::NOT_FOUND, "Task not found"));
    }

    let build_targets = BuildTargetsRepo::list_by_task_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch build targets for task {}: {}", task_id, e);
            message_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    Ok(Json(
        build_targets
            .into_iter()
            .map(|build_target| BuildTargetDTO {
                id: build_target.id.to_string(),
                task_id: build_target.task_id.to_string(),
                path: build_target.path,
                latest_state: build_target.latest_state,
            })
            .collect(),
    ))
}

pub async fn build_logs(
    state: &AppState,
    build_id: &str,
) -> Result<Json<LogLinesResponse>, (StatusCode, Json<LogErrorResponse>)> {
    let build_uuid = build_id.parse::<Uuid>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(LogErrorResponse {
                message: "Invalid build ID".to_string(),
            }),
        )
    })?;

    let build_event = BuildEventsRepo::find_by_id(&state.conn, build_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch build event {}: {}", build_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogErrorResponse {
                    message: "Database error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(LogErrorResponse {
                    message: "Build event not found".to_string(),
                }),
            )
        })?;

    // `log_output_file` is the authoritative artifact location recorded at build
    // creation; read from it directly rather than re-deriving the key.
    let log_content = state
        .log_service
        .read_full_log_by_key(&build_event.log_output_file)
        .await
        .map_err(|e| {
            tracing::error!("Failed to read log for build {}: {}", build_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogErrorResponse {
                    message: "Failed to read log".to_string(),
                }),
            )
        })?;

    let lines: Vec<String> = log_content.lines().map(str::to_string).collect();
    Ok(Json(LogLinesResponse {
        len: lines.len(),
        data: lines,
    }))
}

pub async fn build_state(
    state: &AppState,
    build_id: &str,
) -> Result<Json<BuildStatus>, MessageErrorResponse> {
    let build_uuid = parse_uuid_or_message_error(build_id, "Invalid build ID")?;
    if state.scheduler.active_builds.contains_key(build_id) {
        return Ok(Json(BuildStatus::Running));
    }

    let build_event = BuildEventsRepo::find_by_id(&state.conn, build_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch build event {}: {}", build_id, e);
            message_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?
        .ok_or_else(|| message_error(StatusCode::NOT_FOUND, "Build not found"))?;

    let state_enum = match (build_event.end_at, build_event.exit_code) {
        (None, _) => BuildStatus::Running,
        (Some(_), Some(0)) => BuildStatus::Completed,
        (Some(_), Some(_)) => BuildStatus::Failed,
        (Some(_), None) => BuildStatus::Failed,
    };
    Ok(Json(state_enum))
}

pub async fn latest_build_result(
    state: &AppState,
    task_id: &str,
) -> Result<Json<BuildStatus>, MessageErrorResponse> {
    let task_uuid = parse_uuid_or_message_error(task_id, "Invalid task ID")?;
    let task_exists = task_exists_by_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify task existence {}: {}", task_id, e);
            message_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;
    if !task_exists {
        return Err(message_error(StatusCode::NOT_FOUND, "Task not found"));
    }

    let latest_build_event = BuildEventsRepo::latest_by_task_id(&state.conn, task_uuid)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to fetch latest build event for task {}: {}",
                task_id,
                e
            );
            message_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;

    let build_event = latest_build_event.ok_or_else(|| {
        message_error(StatusCode::NOT_FOUND, "No build events found for this task")
    })?;

    let state_enum = match (build_event.end_at, build_event.exit_code) {
        (None, _) => BuildStatus::Running,
        (Some(_), Some(0)) => BuildStatus::Completed,
        (Some(_), Some(_)) => BuildStatus::Failed,
        (Some(_), None) => BuildStatus::Failed,
    };
    Ok(Json(state_enum))
}

pub async fn queue_stats(state: &AppState) -> (StatusCode, Json<TaskQueueStats>) {
    let stats = state.scheduler.get_queue_stats().await;
    (StatusCode::OK, Json(stats))
}

pub async fn health_check(state: &AppState) -> (StatusCode, Json<Value>) {
    match OrionTasksRepo::ping(&state.conn).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status": "healthy"}))),
        Err(e) => {
            tracing::error!("Health check failed: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status": "unhealthy", "error": "database connectivity check failed"})),
            )
        }
    }
}

pub async fn task_history_output(
    state: &AppState,
    params: &TaskHistoryQuery,
) -> Result<Json<LogLinesResponse>, (StatusCode, Json<LogErrorResponse>)> {
    let log_result = if matches!((params.start, params.end), (None, None)) {
        state
            .log_service
            .read_full_log(&params.task_id, &params.repo, &params.build_id)
            .await
    } else {
        let start = params.start.unwrap_or(0);
        let end = params.end.unwrap_or(usize::MAX);
        state
            .log_service
            .read_log_range(&params.task_id, &params.repo, &params.build_id, start, end)
            .await
    };

    let log_content = match log_result {
        Ok(content) => content,
        Err(e) => {
            tracing::error!("read log failed: {:?}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogErrorResponse {
                    message: "Failed to read log file".to_string(),
                }),
            ));
        }
    };

    let lines: Vec<String> = log_content.lines().map(str::to_string).collect();
    Ok(Json(LogLinesResponse {
        len: lines.len(),
        data: lines,
    }))
}

pub async fn target_logs(
    state: &AppState,
    target_id: &str,
    params: &TargetLogQuery,
) -> Result<Json<TargetLogLinesResponse>, (StatusCode, Json<LogErrorResponse>)> {
    let target_uuid = target_id.parse::<Uuid>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(LogErrorResponse {
                message: "Invalid target id".to_string(),
            }),
        )
    })?;

    let build_target = match BuildTargetsRepo::find_by_id(&state.conn, target_uuid).await {
        Ok(Some(target)) => target,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(LogErrorResponse {
                    message: "Target not found".to_string(),
                }),
            ));
        }
        Err(err) => {
            tracing::error!("Failed to load target {}: {}", target_id, err);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogErrorResponse {
                    message: "Failed to read target".to_string(),
                }),
            ));
        }
    };

    let build_event = if let Some(build_id) = params.build_id.as_ref() {
        let build_uuid = build_id.parse::<Uuid>().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(LogErrorResponse {
                    message: "Invalid build id".to_string(),
                }),
            )
        })?;

        match BuildEventsRepo::find_by_id(&state.conn, build_uuid).await {
            Ok(Some(build)) if build.task_id == build_target.task_id => build,
            Ok(Some(_)) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(LogErrorResponse {
                        message: "Build does not belong to this target".to_string(),
                    }),
                ));
            }
            Ok(None) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(LogErrorResponse {
                        message: "Build not found".to_string(),
                    }),
                ));
            }
            Err(err) => {
                tracing::error!("Failed to load build {}: {}", build_uuid, err);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LogErrorResponse {
                        message: "Failed to load build".to_string(),
                    }),
                ));
            }
        }
    } else {
        match BuildEventsRepo::latest_by_task_id(&state.conn, build_target.task_id).await {
            Ok(Some(build)) => build,
            Ok(None) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(LogErrorResponse {
                        message: "No builds for target".to_string(),
                    }),
                ));
            }
            Err(err) => {
                tracing::error!("Failed to load build for target {}: {}", target_uuid, err);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LogErrorResponse {
                        message: "Failed to load build".to_string(),
                    }),
                ));
            }
        }
    };

    // Read from the authoritative artifact key recorded on the build event.
    let log_key = &build_event.log_output_file;
    let log_result = if matches!(params.r#type, LogReadMode::Segment) {
        let offset = params.offset.unwrap_or(0);
        let limit = params.limit.unwrap_or(200);
        state
            .log_service
            .read_log_range_by_key(log_key, offset, offset + limit)
            .await
    } else {
        state.log_service.read_full_log_by_key(log_key).await
    };

    match log_result {
        Ok(content) => {
            let lines: Vec<String> = content.lines().map(str::to_string).collect();
            Ok(Json(TargetLogLinesResponse {
                len: lines.len(),
                data: lines,
                build_id: build_event.id.to_string(),
            }))
        }
        Err(e) => {
            tracing::error!(
                "Failed to read logs for target {} build {}: {}",
                target_uuid,
                build_event.id,
                e
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogErrorResponse {
                    message: "Failed to read log".to_string(),
                }),
            ))
        }
    }
}

pub async fn build_retry(
    state: &AppState,
    req: api_model::buck2::api::RetryBuildRequest,
) -> Response {
    let old_build_id = match req.build_id.parse::<uuid::Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"message": "Invalid build ID format"})),
            )
                .into_response();
        }
    };

    if state.scheduler.active_builds.contains_key(&req.build_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"message": "The build already exists"})),
        )
            .into_response();
    }

    let old_event = match BuildEventsRepo::find_by_id(&state.conn, old_build_id).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"message": "Build event not found"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": "Database find failed"})),
            )
                .into_response();
        }
    };

    let task = match OrionTasksRepo::find_by_id(&state.conn, old_event.task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"message": "Task not found"})),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": "Database find failed"})),
            )
                .into_response();
        }
    };

    // Only the latest build of a task may be retried. Retrying a superseded build
    // would fork history and can spawn duplicate concurrent builds.
    match BuildEventsRepo::latest_by_task_id(&state.conn, old_event.task_id).await {
        Ok(Some(latest)) if latest.id != old_event.id => {
            return (
                StatusCode::CONFLICT,
                Json(json!({"message": "Only the latest build of the task can be retried"})),
            )
                .into_response();
        }
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": "Database find failed"})),
            )
                .into_response();
        }
    }

    // A build can only be retried once it has finished; an unfinished build means
    // the task still has work in flight (queued or building).
    if old_event.end_at.is_none() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"message": "Build is still in progress"})),
        )
            .into_response();
    }

    let retry_count = old_event.retry_count + 1;
    let repo = task.repo_name.clone();
    let cl_link = task.cl.clone();
    // Retry requests do not carry repo, so normalize against the persisted task repo.
    let changes = normalize_repo_root_changes(&repo, req.changes);

    let build_target =
        match BuildTargetsRepo::ensure_any_target_for_task(&state.conn, task.id).await {
            Ok(t) => t,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"message": "Database find failed"})),
                )
                    .into_response();
            }
        };

    let new_build_id = Uuid::now_v7();

    // If no idle workers, do not queue: create the build and immediately mark it
    // Interrupted so it surfaces as a clear, retryable terminal state.
    if !state.scheduler.has_idle_workers() {
        if let Err(e) =
            BuildEventsRepo::insert_build(&state.conn, new_build_id, task.id, repo.clone()).await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": format!("Failed to create build event: {e}")})),
            )
                .into_response();
        }

        state
            .scheduler
            .fail_unschedulable_build(new_build_id, task.id, &repo)
            .await;

        (
            StatusCode::OK,
            Json(json!({
                "message": "No build worker available; build marked as Interrupted",
                "build_id": new_build_id.to_string(),
            })),
        )
            .into_response()
    } else {
        if let Err(e) =
            BuildEventsRepo::insert_build(&state.conn, new_build_id, task.id, repo.clone()).await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"message": format!("Failed to create build event: {e}")})),
            )
                .into_response();
        }

        // Dispatch immediately to a chosen worker.
        let idle_workers = state.scheduler.get_idle_workers();
        let chosen_index = {
            let mut rng = rand::rng();
            rng.random_range(0..idle_workers.len())
        };
        let chosen_id = idle_workers[chosen_index].clone();

        let started_at = chrono::Utc::now();
        let event_payload = BuildEventPayload::new(
            new_build_id,
            task.id,
            cl_link.clone(),
            repo.clone(),
            retry_count,
        );
        let build_info = BuildInfo {
            event_payload: event_payload.clone(),
            target_id: build_target.id,
            target_path: build_target.path.clone(),
            changes: changes.clone(),
            worker_id: chosen_id.clone(),
            started_at,
        };

        let msg = api_model::buck2::ws::WSMessage::TaskBuild {
            build_id: new_build_id.to_string(),
            repo: repo.clone(),
            changes,
            cl_link,
        };

        // Claim + send under DashMap write guard, then drop it BEFORE DB awaits.
        // Holding `workers.get_mut` across `.await` can stall the tokio runtime
        // (heartbeats / health checks / `/orion-clients-info` block on the shard).
        if let Err(e) = state
            .scheduler
            .claim_worker_for_build(&chosen_id, build_info, msg)
        {
            return (StatusCode::BAD_GATEWAY, Json(json!({"message": e}))).into_response();
        }

        let now_tz = started_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap());
        let _ = BuildTargetsRepo::update_latest_state(
            &state.conn,
            build_target.id,
            TargetState::Building,
        )
        .await;
        let _ = TargetStateHistoriesRepo::upsert_state(
            &state.conn,
            build_target.id,
            new_build_id,
            TargetState::Building.to_string(),
            now_tz,
        )
        .await;

        (
            StatusCode::OK,
            Json(json!({"message": "Build retry dispatched immediately to worker"})),
        )
            .into_response()
    }
}

async fn activate_worker(
    build_info: &BuildInfo,
    scheduler: &crate::scheduler::TaskScheduler,
) -> OrionBuildResult {
    let msg = WSMessage::TaskBuild {
        build_id: build_info.event_payload.build_event_id.to_string(),
        repo: build_info.event_payload.repo.clone(),
        changes: build_info.changes.clone(),
        cl_link: build_info.event_payload.cl_link.clone(),
    };
    let key = build_info.event_payload.build_event_id.to_string();
    if scheduler
        .claim_worker_for_build(&build_info.worker_id, build_info.clone(), msg)
        .is_ok()
    {
        return OrionBuildResult {
            build_id: key,
            status: "dispatched".to_string(),
            message: format!("Build dispatched to worker {}", build_info.worker_id),
        };
    }
    scheduler.release_worker(&build_info.worker_id).await;
    OrionBuildResult {
        build_id: build_info.event_payload.build_event_id.to_string(),
        status: "error".to_string(),
        message: "Failed to dispatch task to worker".to_string(),
    }
}

async fn handle_immediate_task_dispatch_v2(
    state: &AppState,
    task_id: Uuid,
    repo: &str,
    cl_link: &str,
    changes: Vec<Status<ProjectRelativePath>>,
) -> OrionBuildResult {
    let build_event_id = Uuid::now_v7();
    let Some(chosen_id) = state
        .scheduler
        .search_and_claim_worker(&build_event_id.to_string())
    else {
        return OrionBuildResult {
            build_id: "".to_string(),
            status: "error".to_string(),
            message: "No available workers at the moment".to_string(),
        };
    };

    let target_id = Uuid::now_v7();
    let target_path =
        match BuildTargetsRepo::insert_default_target(target_id, task_id, &state.conn).await {
            Ok(default_path) => default_path,
            Err(_) => {
                state.scheduler.release_worker(&chosen_id).await;
                return OrionBuildResult {
                    build_id: "".to_string(),
                    status: "error".to_string(),
                    message: format!("Failed to prepare target for task {}", task_id),
                };
            }
        };

    let event = BuildEventPayload::new(
        build_event_id,
        task_id,
        cl_link.to_string(),
        repo.to_string(),
        0,
    );
    let build_info = BuildInfo {
        event_payload: event.clone(),
        changes: changes.clone(),
        target_id,
        target_path,
        worker_id: chosen_id,
        started_at: chrono::Utc::now(),
    };

    if let Err(e) =
        BuildEventsRepo::insert_build(&state.conn, build_event_id, task_id, repo.to_string()).await
    {
        state.scheduler.release_worker(&build_info.worker_id).await;
        return OrionBuildResult {
            build_id: "".to_string(),
            status: "error".to_string(),
            message: format!(
                "Failed to insert build event into database for task {}: {}",
                task_id, e
            ),
        };
    }

    // Move the target to `Building` so the UI shows an active build instead of
    // leaving it stuck in the queued (`Uninitialized`) state. The queued dispatch
    // path (`scheduler::dispatch_task_v2`) already does this; the immediate
    // dispatch path must do the same.
    let now_tz = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(0).unwrap_or_else(|| unreachable!()));
    let _ =
        BuildTargetsRepo::update_latest_state(&state.conn, target_id, TargetState::Building).await;
    let _ = TargetStateHistoriesRepo::upsert_state(
        &state.conn,
        target_id,
        build_event_id,
        TargetState::Building.to_string(),
        now_tz,
    )
    .await;

    activate_worker(&build_info, &state.scheduler).await
}

/// Create a build event + default target for a task that cannot be scheduled
/// (no idle worker) and immediately mark it `Interrupted`. Returns the new build
/// id so callers can report it to the client.
async fn reject_unschedulable_build(
    state: &AppState,
    task_id: Uuid,
    repo: &str,
) -> Result<Uuid, sea_orm::DbErr> {
    let build_event_id = Uuid::now_v7();
    BuildEventsRepo::insert_build(&state.conn, build_event_id, task_id, repo.to_string()).await?;

    let target_id = Uuid::now_v7();
    BuildTargetsRepo::insert_default_target(target_id, task_id, &state.conn).await?;

    state
        .scheduler
        .fail_unschedulable_build(build_event_id, task_id, repo)
        .await;

    Ok(build_event_id)
}

pub async fn task_handler_v2(state: &AppState, req: TaskBuildRequest) -> Response {
    let req = TaskBuildRequest {
        changes: normalize_repo_root_changes(&req.repo, req.changes),
        ..req
    };
    let task_id = Uuid::now_v7();
    if let Err(err) =
        OrionTasksRepo::insert_task(task_id, &req.cl_link, &req.repo, &req.changes, &state.conn)
            .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"message": format!("Failed to insert task into database: {}", err)})),
        )
            .into_response();
    }
    let result = if state.scheduler.has_idle_workers() {
        handle_immediate_task_dispatch_v2(state, task_id, &req.repo, &req.cl_link, req.changes)
            .await
    } else {
        // No worker is available: instead of queuing (which can leave a build
        // stuck), create the build and immediately mark it Interrupted so the UI
        // shows a clear, retryable terminal state.
        match reject_unschedulable_build(state, task_id, &req.repo).await {
            Ok(build_id) => OrionBuildResult {
                build_id: build_id.to_string(),
                status: "interrupted".to_string(),
                message: "No build worker available; build marked as Interrupted".to_string(),
            },
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"message": format!("Unable to record build: {}", e)})),
                )
                    .into_response();
            }
        }
    };
    (
        StatusCode::OK,
        Json(OrionServerResponse {
            task_id: task_id.to_string(),
            results: vec![result],
        }),
    )
        .into_response()
}

pub async fn get_orion_clients_info(
    state: &AppState,
    params: PageParams<OrionClientQuery>,
) -> Result<Json<CommonPage<OrionClientInfo>>, (StatusCode, Json<Value>)> {
    let pagination = params.pagination;
    let query = params.additional.clone();
    let page = pagination.page.max(1);
    let per_page = pagination.per_page.clamp(1u64, 100);
    let offset = (page - 1) * per_page;
    let mut total: u64 = 0;
    let mut items: Vec<OrionClientInfo> = Vec::with_capacity(per_page as usize);

    for entry in state.scheduler.workers.iter() {
        let matches = query
            .hostname
            .as_ref()
            .is_none_or(|h| entry.value().hostname.contains(h))
            && query
                .status
                .as_ref()
                .is_none_or(|s| entry.value().status.status_type() == *s)
            && query.phase.as_ref().is_none_or(|p| {
                matches!(
                    entry.value().status,
                    WorkerStatus::Busy { phase: Some(ref x), .. } if *x == *p
                )
            });
        if matches {
            total += 1;
            if total > offset && items.len() < per_page as usize {
                items.push(OrionClientInfo {
                    client_id: entry.key().clone(),
                    hostname: entry.value().hostname.clone(),
                    orion_version: entry.value().orion_version.clone(),
                    start_time: entry.value().start_time,
                    last_heartbeat: entry.value().last_heartbeat,
                });
            }
        }
    }
    Ok(Json(CommonPage { total, items }))
}

pub async fn get_orion_client_status_by_id(
    state: &AppState,
    id: &str,
) -> Result<Json<OrionClientStatus>, (StatusCode, Json<Value>)> {
    let worker = state.scheduler.workers.get(id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"message": "Orion client not found"})),
        )
    })?;
    Ok(Json(OrionClientStatus::from_worker_status(&worker)))
}

#[cfg(test)]
mod tests {
    use api_model::buck2::{status::Status, types::ProjectRelativePath};

    use super::normalize_repo_root_changes;

    #[test]
    fn test_normalize_repo_root_changes_trims_leading_slashes_and_deduplicates() {
        let normalized = normalize_repo_root_changes(
            "/",
            vec![
                Status::Modified(ProjectRelativePath::new("/jupiter/callisto/src/main.rs")),
                Status::Modified(ProjectRelativePath::new("jupiter/callisto/src/main.rs")),
                Status::Removed(ProjectRelativePath::new("//common/lib.rs")),
                Status::Removed(ProjectRelativePath::new("common/lib.rs")),
            ],
        );

        assert_eq!(
            normalized,
            vec![
                Status::Modified(ProjectRelativePath::new("jupiter/callisto/src/main.rs")),
                Status::Removed(ProjectRelativePath::new("common/lib.rs")),
            ]
        );
    }

    #[test]
    fn test_normalize_repo_root_changes_keeps_distinct_status_entries() {
        let normalized = normalize_repo_root_changes(
            "/",
            vec![
                Status::Added(ProjectRelativePath::new("/common/lib.rs")),
                Status::Removed(ProjectRelativePath::new("common/lib.rs")),
            ],
        );

        assert_eq!(
            normalized,
            vec![
                Status::Added(ProjectRelativePath::new("common/lib.rs")),
                Status::Removed(ProjectRelativePath::new("common/lib.rs")),
            ]
        );
    }

    #[test]
    fn test_normalize_repo_root_changes_strips_repo_prefix_for_local_paths() {
        let normalized = normalize_repo_root_changes(
            "/project/buck2_test",
            vec![
                Status::Modified(ProjectRelativePath::new("src/main.rs")),
                Status::Modified(ProjectRelativePath::new("/src/main.rs")),
                Status::Modified(ProjectRelativePath::new("project/buck2_test/src/main.rs")),
                Status::Added(ProjectRelativePath::new(
                    "/project/buck2_test/src/generated.rs",
                )),
            ],
        );

        assert_eq!(
            normalized,
            vec![
                Status::Modified(ProjectRelativePath::new("src/main.rs")),
                Status::Added(ProjectRelativePath::new("src/generated.rs")),
            ]
        );
    }

    #[test]
    fn test_normalize_repo_root_changes_keeps_external_shared_paths() {
        let normalized = normalize_repo_root_changes(
            "/project/buck2_test",
            vec![Status::Modified(ProjectRelativePath::new("common/lib.rs"))],
        );

        assert_eq!(
            normalized,
            vec![Status::Modified(ProjectRelativePath::new("common/lib.rs"))]
        );
    }

    #[test]
    fn test_normalize_repo_root_changes_filters_unsafe_paths() {
        let normalized = normalize_repo_root_changes(
            "/project/buck2_test",
            vec![
                Status::Modified(ProjectRelativePath::new("src/main.rs")),
                Status::Added(ProjectRelativePath::new("../escape.rs")),
                Status::Removed(ProjectRelativePath::new("project//buck2_test/src/main.rs")),
            ],
        );

        assert_eq!(
            normalized,
            vec![Status::Modified(ProjectRelativePath::new("src/main.rs"))]
        );
    }
}
