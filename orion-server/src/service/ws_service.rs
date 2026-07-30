use std::{net::SocketAddr, ops::ControlFlow};

use api_model::buck2::{types::LogEvent, ws::WSMessage};
use axum::{
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{self, UnboundedSender};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    log::log_service::LogService,
    model::target_state::TargetState,
    repository::{
        build_events_repo::BuildEventsRepo, build_targets_repo::BuildTargetsRepo,
        target_state_histories_repo::TargetStateHistoriesRepo,
    },
    scheduler::{WorkerInfo, WorkerStatus},
};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("{addr} connected. Waiting for registration...");
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, state: AppState) {
    let (tx, mut rx) = mpsc::unbounded_channel::<WSMessage>();
    let mut worker_id: Option<String> = None;
    let (mut sender, mut receiver) = socket.split();

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let msg_str = serde_json::to_string(&msg).unwrap_or_default();
            if sender
                .send(Message::Text(Utf8Bytes::from(msg_str)))
                .await
                .is_err()
            {
                tracing::warn!("Failed to send message to {who}, client disconnected.");
                break;
            }
        }
    });

    let state_clone = state.clone();
    let tx_clone = tx.clone();
    let recv_task = tokio::spawn(async move {
        let mut worker_id_inner: Option<String> = None;
        while let Some(Ok(msg)) = receiver.next().await {
            if process_message(msg, who, &state_clone, &mut worker_id_inner, &tx_clone)
                .await
                .is_break()
            {
                break;
            }
        }
        worker_id_inner
    });

    tokio::select! {
        _ = send_task => {},
        result = recv_task => {
            if let Ok(final_worker_id) = result {
                worker_id = final_worker_id;
            }
        }
    }

    if let Some(id) = worker_id {
        tracing::info!("Cleaning up for worker: {id} from {who}.");
        if let Some((_, info)) = state.scheduler.workers.remove(&id) {
            state
                .scheduler
                .handle_worker_gone(info, "disconnected")
                .await;
        }
    } else {
        tracing::info!("Cleaning up unregistered connection from {who}.");
    }
    tracing::info!("Websocket context {who} destroyed");
}

async fn process_message(
    msg: Message,
    who: SocketAddr,
    state: &AppState,
    worker_id: &mut Option<String>,
    tx: &UnboundedSender<WSMessage>,
) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            let ws_msg: Result<WSMessage, _> = serde_json::from_str(&t);
            if let Err(e) = ws_msg {
                tracing::warn!("Failed to parse message from {who}: {e}");
                return ControlFlow::Continue(());
            }
            let ws_msg = ws_msg.unwrap_or(WSMessage::Heartbeat);

            if worker_id.is_none() {
                if let WSMessage::Register {
                    id,
                    hostname,
                    orion_version,
                } = ws_msg
                {
                    tracing::info!("Worker from {who} registered as: {id}");
                    state.scheduler.workers.insert(
                        id.clone(),
                        WorkerInfo {
                            sender: tx.clone(),
                            status: WorkerStatus::Idle,
                            last_heartbeat: chrono::Utc::now(),
                            start_time: chrono::Utc::now(),
                            hostname,
                            orion_version,
                        },
                    );
                    *worker_id = Some(id);
                    state.scheduler.notify_task_available();
                } else {
                    tracing::error!(
                        "First message from {who} was not Register. Closing connection."
                    );
                    return ControlFlow::Break(());
                }
                return ControlFlow::Continue(());
            }

            let current_worker_id = worker_id.as_ref().unwrap_or(&String::new()).clone();
            match ws_msg {
                WSMessage::Register { .. } => {
                    tracing::warn!(
                        "Worker {current_worker_id} sent Register message again. Ignoring."
                    );
                }
                WSMessage::Heartbeat => {
                    if let Some(mut worker) = state.scheduler.workers.get_mut(&current_worker_id) {
                        worker.last_heartbeat = chrono::Utc::now();
                        if let WorkerStatus::Error(_) = worker.status {
                            worker.status = WorkerStatus::Idle;
                        }
                    }
                }
                WSMessage::TaskAck {
                    build_id,
                    success,
                    message,
                } => {
                    tracing::info!(
                        "TaskAck from worker {current_worker_id}: build_id={build_id} success={success} message={message}"
                    );
                }
                WSMessage::TaskBuildOutput { build_id, output } => {
                    // Resolve metadata and drop the DashMap guard before awaiting
                    // to avoid holding a shard lock across an await point.
                    let meta = state
                        .scheduler
                        .active_builds
                        .get(&build_id)
                        .map(|build_info| {
                            (
                                build_info.event_payload.task_id.to_string(),
                                LogService::last_segment(&build_info.event_payload.repo),
                            )
                        });

                    if let Some((task_id, repo_name)) = meta {
                        // Reliable persistence on the build-output path itself.
                        if let Err(e) = state
                            .log_service
                            .append_local(&task_id, &repo_name, &build_id, &output)
                            .await
                        {
                            tracing::error!(
                                "failed to persist build output, build_id={}, error={:?}",
                                build_id,
                                e
                            );
                        }

                        // Broadcast for live (SSE) viewers only.
                        state.log_service.publish(LogEvent {
                            task_id,
                            repo_name,
                            build_id: build_id.clone(),
                            line: output.clone(),
                            is_end: false,
                        });
                    }
                }
                WSMessage::TaskBuildCompleteV2 {
                    build_id,
                    success,
                    exit_code,
                    message,
                }
                | WSMessage::TaskBuildComplete {
                    build_id,
                    success,
                    exit_code,
                    message,
                } => {
                    let (repo, task_id, target_id) =
                        if let Some(build_info) = state.scheduler.active_builds.get(&build_id) {
                            (
                                build_info.event_payload.repo.clone(),
                                build_info.event_payload.task_id,
                                build_info.target_id,
                            )
                        } else {
                            return ControlFlow::Continue(());
                        };

                    state.log_service.publish(LogEvent {
                        task_id: task_id.to_string(),
                        repo_name: LogService::last_segment(&repo).to_string(),
                        build_id: build_id.to_string(),
                        line: String::new(),
                        is_end: true,
                    });
                    state.scheduler.active_builds.remove(&build_id);

                    let _ = BuildEventsRepo::update_build_complete_result(
                        &build_id,
                        exit_code,
                        &state.conn,
                    )
                    .await;

                    // The build has finished, so the local log is now final.
                    // Mark it complete so reads trust the local copy, then upload
                    // it to cloud storage in the background with retries.
                    let repo_segment = LogService::last_segment(&repo);
                    state
                        .log_service
                        .mark_local_complete(&task_id.to_string(), &repo_segment, &build_id)
                        .await;
                    state.log_service.spawn_cloud_upload(
                        task_id.to_string(),
                        repo_segment,
                        build_id.to_string(),
                    );

                    let target_state = match (success, exit_code) {
                        (true, Some(0)) => TargetState::Completed,
                        (_, None) => TargetState::Interrupted,
                        _ => TargetState::Failed,
                    };
                    let now = chrono::Utc::now().with_timezone(
                        &chrono::FixedOffset::east_opt(0).unwrap_or_else(|| unreachable!()),
                    );
                    let build_event_uuid = build_id.parse::<Uuid>().unwrap_or_else(|_| Uuid::nil());

                    let _ =
                        BuildTargetsRepo::update_latest_state(&state.conn, target_id, target_state)
                            .await;
                    let _ = TargetStateHistoriesRepo::upsert_state(
                        &state.conn,
                        target_id,
                        build_event_uuid,
                        target_state.to_string(),
                        now,
                    )
                    .await;

                    if let Some(mut worker) = state.scheduler.workers.get_mut(&current_worker_id) {
                        worker.status = if success {
                            WorkerStatus::Idle
                        } else {
                            WorkerStatus::Error(message)
                        };
                    }
                    state.scheduler.notify_task_available();
                }
                WSMessage::TaskPhaseUpdate { build_id, phase } => {
                    if let Some(mut worker) = state.scheduler.workers.get_mut(&current_worker_id)
                        && let WorkerStatus::Busy { build_id: id, .. } = &worker.status
                        && &build_id == id
                    {
                        worker.status = WorkerStatus::Busy {
                            build_id,
                            phase: Some(phase),
                        };
                    }
                }
                WSMessage::TargetBuildStatusBatch { events } => {
                    for update in events {
                        state.target_status_cache.insert_event(update).await;
                    }
                }
                _ => {}
            }
        }
        Message::Close(_) => {
            // Do not clear worker_id or overwrite Busy → Lost here: the outer
            // handle_socket cleanup removes the worker and finalizes any in-flight
            // build via handle_worker_gone. Overwriting Busy would lose build_id
            // and leave the build stuck as Building forever.
            tracing::info!("WebSocket Close from {who}");
            return ControlFlow::Break(());
        }
        _ => {}
    }
    ControlFlow::Continue(())
}
