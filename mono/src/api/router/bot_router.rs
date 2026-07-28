use anyhow::anyhow;
use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use ceres::model::bots::{
    BootstrapInitBotResponse, BotRes, ChangeInstallationStatus, CreateBotTokenRequest,
    CreateBotTokenResponse, InstallBotReq, InstallationTargetType, ListBotTokenItem,
};
use chrono::{Duration, Utc};
use jupiter::sea_orm::prelude::DateTimeWithTimeZone;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::group_permission::ensure_admin, api_doc::BOT_TAG,
    error::ApiError, oauth::model::LoginUser,
};

/// Maximum allowed expires_in in seconds (10 years).
const MAX_EXPIRES_IN_SECS: i64 = 365 * 24 * 3600 * 10;
/// Minimum allowed expires_in in seconds.
const MIN_EXPIRES_IN_SECS: i64 = 1;

/// Env var holding the shared secret that gates `POST /bots/bootstrap-init`.
const INIT_BOOTSTRAP_SECRET_ENV: &str = "MEGA_INIT_BOOTSTRAP_SECRET";
/// Header the mega-init Job / script must send.
const INIT_BOOTSTRAP_SECRET_HEADER: &str = "x-mega-init-secret";
const INIT_BOOTSTRAP_SECRET_MIN_LEN: usize = 32;

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Fail closed unless `MEGA_INIT_BOOTSTRAP_SECRET` is set and matches the request header.
fn ensure_init_bootstrap_secret(headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = match std::env::var(INIT_BOOTSTRAP_SECRET_ENV) {
        Ok(v) => {
            let trimmed = v.trim().to_owned();
            if trimmed.len() < INIT_BOOTSTRAP_SECRET_MIN_LEN {
                tracing::error!(
                    env = INIT_BOOTSTRAP_SECRET_ENV,
                    min_len = INIT_BOOTSTRAP_SECRET_MIN_LEN,
                    "init bootstrap secret is too short; refusing bootstrap-init"
                );
                return Err(ApiError::with_status(
                    StatusCode::UNAUTHORIZED,
                    anyhow!("bootstrap-init is not configured"),
                ));
            }
            trimmed
        }
        Err(_) => {
            tracing::error!(
                env = INIT_BOOTSTRAP_SECRET_ENV,
                "init bootstrap secret is unset; refusing bootstrap-init"
            );
            return Err(ApiError::with_status(
                StatusCode::UNAUTHORIZED,
                anyhow!("bootstrap-init is not configured"),
            ));
        }
    };

    let provided = headers
        .get(INIT_BOOTSTRAP_SECRET_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
        tracing::warn!("bootstrap-init rejected: missing or invalid X-Mega-Init-Secret");
        return Err(ApiError::with_status(
            StatusCode::UNAUTHORIZED,
            anyhow!("Unauthorized"),
        ));
    }
    Ok(())
}

async fn ensure_bot_exists(state: &MonoApiServiceState, bot_id: i64) -> Result<(), ApiError> {
    let bot = state.services().admin().get_bot_by_id(bot_id).await?;
    if bot.is_none() {
        return Err(ApiError::not_found(anyhow!("Bot not found")));
    }
    Ok(())
}

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/bots",
        OpenApiRouter::new()
            .routes(routes!(bootstrap_init_bot))
            .routes(routes!(install_bot))
            .routes(routes!(list_installed_bot))
            .routes(routes!(change_installation_status))
            .routes(routes!(uninstall_bot))
            .routes(routes!(create_bot_token))
            .routes(routes!(list_bot_tokens))
            .routes(routes!(revoke_bot_token))
            .routes(routes!(revoke_all_bot_tokens)),
    )
}

/// Bootstrap mega-init bot + push token.
///
/// Gated by shared secret `MEGA_INIT_BOOTSTRAP_SECRET` via header
/// `X-Mega-Init-Secret` (not LoginUser — for the onprem mega-init Job / CronJob).
/// Creates the `mega-init` bot if needed and returns a fresh `bot_` token for
/// git HTTP push.
#[utoipa::path(
    post,
    path = "/bootstrap-init",
    params(
        ("X-Mega-Init-Secret" = String, Header, description = "Must match MEGA_INIT_BOOTSTRAP_SECRET on mono-engine")
    ),
    responses(
        (status = 200, body = CommonResult<BootstrapInitBotResponse>, content_type = "application/json"),
        (status = 401, description = "Missing/invalid X-Mega-Init-Secret or secret not configured"),
    ),
    tag = BOT_TAG
)]
async fn bootstrap_init_bot(
    State(state): State<MonoApiServiceState>,
    headers: HeaderMap,
) -> Result<Json<CommonResult<BootstrapInitBotResponse>>, ApiError> {
    ensure_init_bootstrap_secret(&headers)?;
    let resp = state.services().admin().ensure_init_bot_token().await?;
    Ok(Json(CommonResult::success(Some(resp))))
}

/// Install bot
#[utoipa::path(
    post,
    params(
        ("id", description = "Bots ID"),
    ),
    path = "/{id}/installations",
    responses(
        (status = 200, body = CommonResult<BotRes>, content_type = "application/json")
    ),
    tag = BOT_TAG
)]
async fn install_bot(
    state: State<MonoApiServiceState>,
    Path(id): Path<i64>,
    Json(json): Json<InstallBotReq>,
) -> Result<Json<CommonResult<BotRes>>, ApiError> {
    let bot = state.services().admin().install_bot(id, json).await?;

    Ok(Json(CommonResult::success(Some(bot))))
}

/// Get installed bot
#[utoipa::path(
    get,
    params(
        ("id", description = "Bots ID"),
    ),
    path = "/{id}/installations",
    responses(
        (status = 200, body = CommonResult<Vec<BotRes>>, content_type = "application/json")
    ),
    tag = BOT_TAG
)]
async fn list_installed_bot(
    state: State<MonoApiServiceState>,
    Path(id): Path<i64>,
) -> Result<Json<CommonResult<Vec<BotRes>>>, ApiError> {
    let models = state.services().admin().list_installed_bots(id).await?;

    Ok(Json(CommonResult::success(Some(models))))
}

#[utoipa::path(
    patch,
    params(
        ("id", description = "Bot ID"),
        ("installation_id", description = "Installation ID"),
    ),
    path = "/{id}/installations/{installation_id}",
    responses(
        (status = 200, body = CommonResult<BotRes>, content_type = "application/json")
    ),
    tag = BOT_TAG
)]
async fn change_installation_status(
    state: State<MonoApiServiceState>,
    Path((id, installation_id)): Path<(i64, i64)>,
    Json(json): Json<ChangeInstallationStatus>,
) -> Result<Json<CommonResult<BotRes>>, ApiError> {
    let model = state
        .services()
        .admin()
        .change_bot_installation_status(id, installation_id, json)
        .await?;

    Ok(Json(CommonResult::success(Some(model))))
}

#[utoipa::path(
    delete,
    params(
        ("id", description = "Bot ID"),
        ("installation_id", description = "Installation ID"),
    ),
    path = "/{id}/installations/{installation_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = BOT_TAG
)]
async fn uninstall_bot(
    state: State<MonoApiServiceState>,
    Path((id, installation_id)): Path<(i64, i64)>,
    Json(target_type): Json<InstallationTargetType>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .admin()
        .uninstall_bot(id, target_type, installation_id)
        .await?;

    Ok(Json(CommonResult::success(Some(
        "Bot uninstalled successfully".to_string(),
    ))))
}

/// POST /api/v1/bots/{bot_id}/tokens
///
/// Create a new bot token. Only admins can perform this operation.
#[utoipa::path(
    post,
    path = "/{bot_id}/tokens",
    request_body = CreateBotTokenRequest,
    params(
        ("bot_id" = i64, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, body = CommonResult<CreateBotTokenResponse>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Bot not found"),
    ),
    tag = BOT_TAG
)]
async fn create_bot_token(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(bot_id): Path<i64>,
    Json(req): Json<CreateBotTokenRequest>,
) -> Result<Json<CommonResult<CreateBotTokenResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    ensure_bot_exists(&state, bot_id).await?;

    let expires_at: Option<DateTimeWithTimeZone> = match req.expires_in {
        None => None,
        Some(seconds) => {
            if !(MIN_EXPIRES_IN_SECS..=MAX_EXPIRES_IN_SECS).contains(&seconds) {
                return Err(ApiError::bad_request(anyhow!(
                    "expires_in must be between {} and {} seconds",
                    MIN_EXPIRES_IN_SECS,
                    MAX_EXPIRES_IN_SECS
                )));
            }
            let when = Utc::now() + Duration::seconds(seconds);
            Some(DateTimeWithTimeZone::from(when))
        }
    };

    let resp = state
        .services()
        .admin()
        .generate_bot_token(bot_id, &req.token_name, expires_at)
        .await?;

    Ok(Json(CommonResult::success(Some(resp))))
}

/// GET /api/v1/bots/{bot_id}/tokens
///
/// List existing tokens for a bot (without plaintext).
#[utoipa::path(
    get,
    path = "/{bot_id}/tokens",
    params(
        ("bot_id" = i64, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, body = CommonResult<Vec<ListBotTokenItem>>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Bot not found"),
    ),
    tag = BOT_TAG
)]
async fn list_bot_tokens(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(bot_id): Path<i64>,
) -> Result<Json<CommonResult<Vec<ListBotTokenItem>>>, ApiError> {
    ensure_admin(&state, &user).await?;
    ensure_bot_exists(&state, bot_id).await?;

    let items = state.services().admin().list_bot_tokens(bot_id).await?;

    Ok(Json(CommonResult::success(Some(items))))
}

/// DELETE /api/v1/bots/{bot_id}/tokens/{id}
///
/// Revoke a single bot token. Idempotent.
#[utoipa::path(
    delete,
    path = "/{bot_id}/tokens/{id}",
    params(
        ("bot_id" = i64, Path, description = "Bot ID"),
        ("id" = i64, Path, description = "Token ID")
    ),
    responses(
        (status = 200, description = "Token revoked successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Bot not found"),
    ),
    tag = BOT_TAG
)]
async fn revoke_bot_token(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((bot_id, token_id)): Path<(i64, i64)>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    ensure_admin(&state, &user).await?;
    ensure_bot_exists(&state, bot_id).await?;

    state
        .services()
        .admin()
        .revoke_bot_token(bot_id, token_id)
        .await?;

    Ok(Json(CommonResult::success(None)))
}

/// POST /api/v1/bots/{bot_id}/tokens/revoke_all
///
/// Revoke all tokens for a given bot. Idempotent.
#[utoipa::path(
    post,
    path = "/{bot_id}/tokens/revoke_all",
    params(
        ("bot_id" = i64, Path, description = "Bot ID")
    ),
    responses(
        (status = 200, description = "All tokens revoked successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Bot not found"),
    ),
    tag = BOT_TAG
)]
async fn revoke_all_bot_tokens(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(bot_id): Path<i64>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    ensure_admin(&state, &user).await?;
    ensure_bot_exists(&state, bot_id).await?;

    state
        .services()
        .admin()
        .revoke_all_bot_tokens(bot_id)
        .await?;

    Ok(Json(CommonResult::success(None)))
}
