use std::{io, ops::ControlFlow, time::Duration};

use api_model::buck2::ws::WSMessage;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{
        mpsc,
        mpsc::{UnboundedReceiver, UnboundedSender},
    },
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_tls_with_config,
    tungstenite::{
        client::IntoClientRequest,
        error::{Error as WsError, UrlError},
        handshake::client::{Request, Response},
        protocol::Message,
    },
};
use url::Url;
use uuid::Uuid;

use crate::api::buck_build;

const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_RESPONSE_MAX_BYTES: usize = 8 * 1024;

/// Manages persistent WebSocket connection with automatic reconnection.
///
/// Handles connection establishment, registration, heartbeat, and task processing.
/// Implements exponential backoff for reconnection attempts.
///
/// # Arguments
/// * `server_addr` - WebSocket server endpoint URL
/// * `worker_id` - Unique identifier for this worker instance
pub async fn run_client(server_addr: String, worker_id: String) {
    let mut reconnect_delay = Duration::from_secs(1);
    const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

    loop {
        tracing::info!("Attempting to connect to server: {}", server_addr);
        match connect_async_maybe_via_proxy(&server_addr).await {
            Ok((ws_stream, response)) => {
                tracing::info!(
                    "WebSocket handshake successful. Server response: {:?}",
                    response.status()
                );
                // Reset reconnect delay after successful connection
                reconnect_delay = Duration::from_secs(1);
                // Handle the active connection
                handle_connection(ws_stream, worker_id.clone(), server_addr.clone()).await;
                tracing::warn!("Disconnected from server.");
            }
            Err(e) => {
                tracing::error!(
                    "WebSocket handshake failed: {}. Retrying in {:?}...",
                    e,
                    reconnect_delay
                );
            }
        }
        // Wait before attempting to reconnect
        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn connect_async_maybe_via_proxy(
    server_addr: &str,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WsError> {
    if let Some(proxy_url) = proxy_url_for(server_addr) {
        tracing::info!(
            "Connecting to WebSocket server via proxy {} -> {}",
            proxy_url,
            server_addr
        );
        return connect_async_via_proxy(server_addr, &proxy_url).await;
    }

    tracing::info!("Connecting to WebSocket server directly: {}", server_addr);
    connect_async_direct(server_addr).await
}

fn proxy_url_for(server_addr: &str) -> Option<String> {
    let secure = server_addr.starts_with("wss://");
    let preferred = if secure { "HTTPS_PROXY" } else { "HTTP_PROXY" };
    std::env::var(preferred)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("ALL_PROXY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn websocket_request(server_addr: &str) -> Result<(Request, String, u16), WsError> {
    let request = server_addr.into_client_request()?;
    let host = request
        .uri()
        .host()
        .ok_or(WsError::Url(UrlError::NoHostName))?
        .to_string();
    let port = request
        .uri()
        .port_u16()
        .or_else(|| match request.uri().scheme_str() {
            Some("wss") => Some(443),
            Some("ws") => Some(80),
            _ => None,
        })
        .ok_or(WsError::Url(UrlError::UnsupportedUrlScheme))?;

    Ok((request, host, port))
}

async fn connect_async_direct(
    server_addr: &str,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WsError> {
    let (request, host, port) = websocket_request(server_addr)?;
    let stream = tokio::time::timeout(
        WS_CONNECT_TIMEOUT,
        TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| {
        WsError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("Timed out connecting directly to {host}:{port}"),
        ))
    })?
    .map_err(WsError::Io)?;
    client_async_tls_with_config(request, stream, None, None).await
}

async fn connect_async_via_proxy(
    server_addr: &str,
    proxy_url: &str,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WsError> {
    let (request, host, port) = websocket_request(server_addr)?;
    let proxy = Url::parse(proxy_url).map_err(|err| {
        WsError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid proxy URL {proxy_url}: {err}"),
        ))
    })?;
    let proxy_host = proxy.host_str().ok_or_else(|| {
        WsError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Proxy URL has no host: {proxy_url}"),
        ))
    })?;
    let proxy_port = proxy.port_or_known_default().ok_or_else(|| {
        WsError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Proxy URL has no port: {proxy_url}"),
        ))
    })?;

    if !proxy.username().is_empty() || proxy.password().is_some() {
        return Err(WsError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "Proxy authentication is not yet supported for WebSocket connections",
        )));
    }

    let mut stream = tokio::time::timeout(
        PROXY_CONNECT_TIMEOUT,
        TcpStream::connect((proxy_host, proxy_port)),
    )
    .await
    .map_err(|_| {
        WsError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("Timed out connecting to proxy {proxy_host}:{proxy_port}"),
        ))
    })?
    .map_err(WsError::Io)?;

    let authority = format!("{host}:{port}");
    let connect_request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n\r\n"
    );
    tokio::time::timeout(
        PROXY_HANDSHAKE_TIMEOUT,
        stream.write_all(connect_request.as_bytes()),
    )
    .await
    .map_err(|_| {
        WsError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("Timed out sending CONNECT request to proxy for {authority}"),
        ))
    })?
    .map_err(WsError::Io)?;

    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if response.len() >= PROXY_RESPONSE_MAX_BYTES {
            return Err(WsError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Proxy CONNECT response exceeded {PROXY_RESPONSE_MAX_BYTES} bytes"),
            )));
        }

        let bytes_read = tokio::time::timeout(PROXY_HANDSHAKE_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| {
                WsError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("Timed out waiting for proxy CONNECT response for {authority}"),
                ))
            })?
            .map_err(WsError::Io)?;

        if bytes_read == 0 {
            return Err(WsError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("Proxy closed connection before CONNECT completed for {authority}"),
            )));
        }
        response.extend_from_slice(&chunk[..bytes_read]);
    }

    let response_text = String::from_utf8_lossy(&response);
    let status_line = response_text.lines().next().unwrap_or_default();
    if !status_line.contains(" 200 ") {
        return Err(WsError::Io(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("Proxy CONNECT failed for {authority}: {status_line}"),
        )));
    }

    client_async_tls_with_config(request, stream, None, None).await
}

/// Processes an established WebSocket connection.
///
/// Coordinates three concurrent tasks:
/// - Heartbeat on a timer with priority over build output
/// - Message sending from internal channels
/// - Message receiving and processing from server
///
/// # Arguments
/// * `ws_stream` - Established WebSocket connection
/// * `worker_id` - Worker identifier for registration
async fn handle_connection(
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    worker_id: String,
    server_addr: String,
) {
    let (ws_sender, mut ws_receiver) = ws_stream.split();
    let (internal_tx, mut internal_rx): (UnboundedSender<WSMessage>, UnboundedReceiver<WSMessage>) =
        mpsc::unbounded_channel();

    let worker_id_clone = worker_id.clone();
    let hostname_clone = server_addr.clone();
    let orion_version = env!("CARGO_PKG_VERSION").to_string();

    let send_task = tokio::spawn(async move {
        // Heartbeats must not sit behind unbounded build output/status batches: the server drops
        // workers after ~90s without a Heartbeat (see orion-server health check).
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));

        tracing::info!("Registering with worker ID: {}", worker_id_clone);
        let register = WSMessage::Register {
            id: worker_id_clone,
            hostname: hostname_clone,
            orion_version,
        };
        let register_str = match serde_json::to_string(&register) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to serialize Register: {}", e);
                return;
            }
        };
        let mut ws_sender = ws_sender;
        if let Err(e) = ws_sender.send(Message::Text(register_str.into())).await {
            tracing::error!(
                "Failed to send Register to server: {}. Terminating send task.",
                e
            );
            return;
        }

        loop {
            tokio::select! {
                biased;
                _ = heartbeat_interval.tick() => {
                    tracing::info!("Sending heartbeat...");
                    match serde_json::to_string(&WSMessage::Heartbeat) {
                        Ok(payload) => {
                            if let Err(e) = ws_sender.send(Message::Text(payload.into())).await {
                                tracing::warn!("Failed to send heartbeat on WebSocket: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to serialize Heartbeat: {}", e);
                        }
                    }
                }
                maybe_msg = internal_rx.recv() => {
                    let Some(msg) = maybe_msg else { break; };
                    match serde_json::to_string(&msg) {
                        Ok(msg_str) => {
                            if let Err(e) = ws_sender.send(Message::Text(msg_str.into())).await {
                                tracing::error!(
                                    "Failed to send message to server: {}. Terminating send task.",
                                    e
                                );
                                break;
                            }
                        }
                        Err(e) => tracing::error!("Failed to serialize WSMessage: {}", e),
                    }
                }
            }
        }
    });

    let internal_tx_clone = internal_tx.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if process_server_message(msg, internal_tx_clone.clone())
                .await
                .is_break()
            {
                break;
            }
        }
    });

    // Wait for any task to complete
    tokio::select! {
        _ = send_task => tracing::info!("Send task finished."),
        _ = recv_task => tracing::info!("Receive task finished."),
    }
}

/// Processes incoming server messages and handles task execution.
///
/// Handles different message types including Task assignments and connection management.
/// For Task messages, spawns build processes and sends acknowledgments.
///
/// # Arguments
/// * `msg` - WebSocket message received from server
/// * `tx` - Channel for sending response messages
///
/// # Returns
/// * `ControlFlow::Continue(())` - Continue message processing
/// * `ControlFlow::Break(())` - Terminate connection
async fn process_server_message(
    msg: Message,
    sender: UnboundedSender<WSMessage>,
) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            match serde_json::from_str::<WSMessage>(&t) {
                Ok(ws_msg) => {
                    tracing::info!("Received message from server: {:?}", ws_msg);
                    match ws_msg {
                        WSMessage::TaskBuild {
                            build_id,
                            repo,
                            cl_link,
                            changes,
                        } => {
                            tracing::info!("Received task: id={}", build_id);

                            let pressure = crate::disk::assess_root_disk();
                            if pressure.should_reject_builds() {
                                tracing::error!(
                                    used_pct = pressure.used_pct(),
                                    "Rejecting task {} due to disk pressure",
                                    build_id
                                );
                                crate::disk::reclaim_under_pressure().await;
                                // Re-check after prune — may have recovered.
                                let after = crate::disk::assess_root_disk();
                                if after.should_reject_builds() {
                                    if let Err(e) = sender.send(WSMessage::TaskAck {
                                        build_id,
                                        success: false,
                                        message: format!(
                                            "disk pressure: root filesystem {}% used (threshold {}%). Free space under /data/scorpio or restart orion-runner after cleanup.",
                                            after.used_pct(),
                                            std::env::var("ORION_DISK_REJECT_PCT").unwrap_or_else(|_| "92".into())
                                        ),
                                    }) {
                                        tracing::error!("Failed to send TaskAck: {}", e);
                                    }
                                    return ControlFlow::Continue(());
                                }
                                tracing::warn!(
                                    used_pct = after.used_pct(),
                                    "Disk pressure cleared after prune; accepting task"
                                );
                            } else if matches!(pressure, crate::disk::DiskPressure::Warn { .. }) {
                                tracing::warn!(
                                    used_pct = pressure.used_pct(),
                                    "Disk usage elevated before accepting task {}",
                                    build_id
                                );
                            }

                            tokio::spawn(async move {
                                let task_id_uuid = match Uuid::parse_str(&build_id) {
                                    Ok(uuid) => uuid,
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to parse task id '{}' as Uuid: {}. Aborting task.",
                                            build_id,
                                            e
                                        );
                                        return;
                                    }
                                };

                                let build_result = buck_build(
                                    task_id_uuid,
                                    cl_link,
                                    repo,
                                    changes,
                                    sender.clone(),
                                )
                                .await;

                                if let Err(e) = sender.send(WSMessage::TaskAck {
                                    build_id,
                                    success: build_result.success,
                                    message: build_result.message.clone(),
                                }) {
                                    tracing::error!("Failed to send TaskAck: {}", e);
                                }
                            });
                        }
                        // Log unexpected message types
                        _ => {
                            tracing::warn!("Received unexpected message from server: {:?}", ws_msg);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Error deserializing message from server: {}", e);
                }
            }
        }
        Message::Close(c) => {
            tracing::warn!("Server sent close frame: {:?}", c);
            return ControlFlow::Break(());
        }
        _ => {} // Ignore Binary, Ping, Pong and other message types
    }
    ControlFlow::Continue(())
}
