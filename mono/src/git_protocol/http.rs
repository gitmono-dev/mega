use std::convert::Infallible;

use anyhow::Result;
use axum::{
    body::Body,
    http::{HeaderValue, Request, Response},
};
use base64::Engine;
use bytes::{Bytes, BytesMut};
use ceres::{
    TransportRuntime,
    infra::pack_stream::into_pack_byte_stream,
    transport::protocol::{PushUserInfo, ServiceType, SmartSession, TransportProtocol, smart},
};
use common::errors::{ProtocolError, mega_to_protocol_error};
use futures::{TryStreamExt, stream};
use http::header::AUTHORIZATION;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;

use crate::{
    api::oauth::{bearer_token_from_authorization_value, login_user_from_mono_access_token},
    git_protocol::InfoRefsParams,
};

// # Discovering Reference
// HTTP clients that support the "smart" protocol (or both the "smart" and "dumb" protocols) MUST
// discover references by making a parameterized request for the info/refs file of the repository.
// The request MUST contain exactly one query parameter, service=$servicename,
// where $servicename MUST be the service name the client wishes to contact to complete the operation.
// The request MUST NOT contain additional query parameters.
pub async fn git_info_refs(
    state: &TransportRuntime,
    params: InfoRefsParams,
    repo_path: std::path::PathBuf,
) -> Result<Response<Body>, ProtocolError> {
    let service_name = params.service.unwrap();
    let service_type = service_name.parse::<ServiceType>().unwrap();
    let session = SmartSession::new(repo_path, service_type, TransportProtocol::Http);
    let pkt_line_stream = session.git_info_refs(state).await?;

    let content_type = format!("application/x-{service_name}-advertisement");
    let response = add_default_header(
        content_type,
        Response::builder()
            .body(Body::from(pkt_line_stream.freeze()))
            .unwrap(),
    );
    Ok(response)
}

fn auth_failed() -> Result<Response<Body>, ProtocolError> {
    let resp = Response::builder()
        .status(401)
        .header(
            http::header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Mega\", Bearer realm=\"Mega\""),
        )
        .body(Body::empty())
        .unwrap();
    Ok(resp)
}

/// Parses Basic Auth header, returning the password (which is the token).
/// The username is ignored since we only care about the token in the password field.
fn basic_auth_password_from_authorization_value(value: &str) -> Option<String> {
    let stripped = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(stripped.trim())
        .ok()?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    // Basic auth format: "username:password"
    Some(decoded_str.split(':').nth(1)?.to_owned())
}

/// Uses [`crate::api::oauth::login_user_from_mono_access_token`] for user access tokens,
/// or bot tokens (`bot_` prefix) via [`jupiter::storage::bots_storage::BotsStorage::find_bot_by_token`].
/// Bot tokens additionally require Write/Admin scope and an Enabled installation
/// covering the receive-pack target path.
/// Supports both Bearer tokens and Basic Auth (with token as password).
async fn git_receive_pack_auth(
    state: &TransportRuntime,
    pack_protocol: &mut SmartSession,
    headers: &http::HeaderMap,
) -> Result<bool, ProtocolError> {
    let auth_header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());

    // Try Bearer token first
    let token = auth_header.and_then(bearer_token_from_authorization_value);

    // If no Bearer token, try Basic Auth (token as password)
    let token = token
        .map(String::from)
        .or_else(|| auth_header.and_then(basic_auth_password_from_authorization_value));

    let Some(token) = token else {
        return Ok(false);
    };

    // Bot tokens are prefixed with `bot_`.
    if token.starts_with("bot_") {
        let bots = state.storage.bots_storage();
        let found = bots
            .find_bot_by_token(&token)
            .await
            .map_err(mega_to_protocol_error)?;
        let Some((bot, _)) = found else {
            return Ok(false);
        };

        let repo_path = pack_protocol.repo_path.to_string_lossy();
        let may_push = bots
            .bot_may_push_to_path(&bot, repo_path.as_ref())
            .await
            .map_err(mega_to_protocol_error)?;
        if !may_push {
            tracing::warn!(
                bot_id = bot.id,
                bot_name = %bot.name,
                repo_path = %repo_path,
                permission_scope = ?bot.permission_scope,
                "bot token rejected for receive-pack: insufficient scope or no enabled installation"
            );
            return Ok(false);
        }

        let username = bot.name;
        pack_protocol.auth.username = Some(username.clone());
        pack_protocol.auth.authenticated_user = Some(PushUserInfo { username });
        return Ok(true);
    }

    let user = login_user_from_mono_access_token(&state.storage.user_storage(), &token)
        .await
        .map_err(mega_to_protocol_error)?;
    let Some(user) = user else {
        return Ok(false);
    };

    let Ok(campsite_user_id) = user.require_campsite_user_id() else {
        tracing::warn!(
            "git receive-pack rejected: access token missing campsite_user_id; regenerate token"
        );
        return Ok(false);
    };
    if user.require_github_login().is_err() {
        tracing::warn!(
            campsite_user_id = %campsite_user_id,
            "git receive-pack rejected: access token missing github_login; regenerate token after GitHub login"
        );
        return Ok(false);
    }
    // Push actor persisted on CLs is campsite_user_id (not github_login).
    let username = campsite_user_id.to_string();
    pack_protocol.auth.username = Some(username.clone());
    pack_protocol.auth.authenticated_user = Some(PushUserInfo { username });
    Ok(true)
}

/// # Handles a Git upload pack request and prepares the response.
///
/// The function takes a `req` parameter representing the HTTP request received and a `pack_protocol`
/// parameter containing the configuration for the Git pack protocol.
///
/// The function extracts the request body into a `BytesMut` buffer by iterating over the chunks
/// of the request body using `body.next().await`. The chunks are concatenated into the `upload_request`
/// buffer.
///
/// The `pack_protocol` is then used to process the `upload_request` using the `git_upload_pack` method.
/// It returns the `send_pack_data` and `buf` containing the response data.
///
/// A response header is constructed using the `build_res_header` function with a content type of
/// "application/x-git-upload-pack-result". The response body channel is created using `Body::channel()`.
///
/// The `buf` is sent as the initial data using the `sender` to establish the response body.
///
/// A new task is spawned to send the remaining `send_pack_data` using the `send_pack` function.
///
/// Finally, the constructed response with the response body is returned.
pub async fn git_upload_pack(
    state: &TransportRuntime,
    req: Request<Body>,
    repo_path: std::path::PathBuf,
) -> Result<Response<Body>, ProtocolError> {
    let mut pack_protocol =
        SmartSession::new(repo_path, ServiceType::UploadPack, TransportProtocol::Http);
    let upload_request: BytesMut = req
        .into_body()
        .into_data_stream()
        .try_fold(BytesMut::new(), |mut acc, chunk| async move {
            acc.extend_from_slice(&chunk);
            Ok(acc)
        })
        .await
        .unwrap();
    tracing::debug!("Receive bytes: <-------- {:?}", upload_request);
    let (mut send_pack_data, protocol_buf) = pack_protocol
        .git_upload_pack(state, &mut upload_request.freeze())
        .await?;

    let body_stream = async_stream::stream! {
        tracing::info!("send ack/nak message buf: --------> {:?}", &protocol_buf);
        yield Ok::<_, Infallible>(Bytes::copy_from_slice(&protocol_buf));
        // send packdata with sideband64k
        while let Some(chunk) = send_pack_data.next().await {
            let mut reader = chunk.as_slice();
            loop {
                let mut temp = BytesMut::new();
                temp.reserve(65500);
                let length = reader.read_buf(&mut temp).await.unwrap();
                if length == 0 {
                    break;
                }
                let bytes_out = pack_protocol.build_side_band_format(temp, length);
                // tracing::info!("send pack file: length: {:?}", bytes_out.len());
                yield Ok::<_, Infallible>(bytes_out.freeze());
            }
        }
        let bytes_out = Bytes::from_static(smart::PKT_LINE_END_MARKER);
        tracing::info!("send back pkt-flush line '0000', actually: {:?}", bytes_out);
        yield Ok::<_, Infallible>(bytes_out);
    };
    let response = add_default_header(
        String::from("application/x-git-upload-pack-result"),
        Response::builder()
            .body(Body::from_stream(body_stream))
            .unwrap(),
    );
    Ok(response)
}

/// Handles the Git receive-pack protocol for receiving and processing data from a client.
///
/// This asynchronous function processes an HTTP request to handle the Git "receive-pack" service,
/// which is used for receiving data when pushing changes to a Git repository. The function reads
/// data from the request body, processes it according to the Git smart protocol, and sends back
/// a response indicating the status of the operation.
///
/// # Parameters
/// - `req`: The incoming HTTP request containing the body stream with the Git data.
/// - `pack_protocol`: A mutable instance of `SmartProtocol` used to process the Git receive-pack protocol.
///
/// # Returns
/// A `Result` containing either:
/// - `Response<Body>`: The HTTP response with the result of the receive-pack operation.
/// - `(StatusCode, String)`: A tuple with an HTTP status code and an error message in case of failure.
pub async fn git_receive_pack(
    state: &TransportRuntime,
    req: Request<Body>,
    repo_path: std::path::PathBuf,
) -> Result<Response<Body>, ProtocolError> {
    let mut pack_protocol =
        SmartSession::new(repo_path, ServiceType::ReceivePack, TransportProtocol::Http);
    if !git_receive_pack_auth(state, &mut pack_protocol, req.headers()).await? {
        return auth_failed();
    }
    // Convert the request body into a data stream.
    let mut data_stream = req.into_body().into_data_stream();
    let mut report_status = Bytes::new();

    let mut chunk_buffer = BytesMut::new(); // Used to cache the data of chunks before the PACK subsequence is found.
    // Process the data stream to handle the Git receive-pack protocol.
    while let Some(chunk) = data_stream.next().await {
        let chunk = chunk.unwrap();
        // Process the data up to the "PACK" subsequence.
        if let Some(pos) = search_subsequence(&chunk, b"PACK") {
            chunk_buffer.extend_from_slice(&chunk[0..pos]);
            let commands =
                pack_protocol.parse_receive_pack_commands(Bytes::copy_from_slice(&chunk_buffer));
            // Create a new stream from the remaining bytes and the rest of the data stream.
            let left_chunk_bytes = Bytes::copy_from_slice(&chunk[pos..]);
            let pack_stream = stream::once(async { Ok(left_chunk_bytes) }).chain(data_stream);
            report_status = pack_protocol
                .git_receive_pack_stream(state, commands, into_pack_byte_stream(pack_stream))
                .await?;
            break;
        } else {
            chunk_buffer.extend_from_slice(&chunk);
        }
    }
    tracing::info!("report status:{:?}", report_status);
    let response = Response::builder().body(Body::from(report_status)).unwrap();
    let response = add_default_header(
        String::from("application/x-git-receive-pack-result"),
        response,
    );
    Ok(response)
}

// Function to find the subsequence in a slice
pub fn search_subsequence(chunk: &[u8], search: &[u8]) -> Option<usize> {
    chunk.windows(search.len()).position(|s| s == search)
}

/// # Build Response headers for Smart Server.
/// Clients MUST NOT reuse or revalidate a cached response.
/// Servers MUST include sufficient Cache-Control headers to prevent caching of the response.
fn add_default_header<T>(content_type: String, mut response: Response<T>) -> Response<T> {
    response.headers_mut().insert(
        "Content-Type",
        HeaderValue::from_str(&content_type).unwrap(),
    );
    response.headers_mut().insert(
        "Cache-Control",
        HeaderValue::from_static("no-cache, max-age=0, must-revalidate"),
    );
    response
}

#[cfg(test)]
mod tests {}
