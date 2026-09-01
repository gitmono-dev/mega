use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
    routing::get,
};
use ceres::model::{
    notification::{
        NotificationEventTypeInfo, UpdateUserNotificationConfig, UserNotificationConfig,
    },
    serde_snowflake::SnowflakeId,
    user::{
        AddSSHKey, ClaContentRes, ClaSignStatusRes, ListSSHKey, ListToken, UpdateClaContentPayload,
        UserApprovalStatusRes,
    },
};
use common::errors::MegaError;
use russh::keys::{HashAlg, parse_public_key_base64};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_common::identity::{collaboration_actor, collaboration_github_login},
    api_doc::USER_TAG,
    error::ApiError,
    oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/user",
        OpenApiRouter::new()
            .route("/", get(user))
            .routes(routes!(list_key))
            .routes(routes!(add_key))
            .routes(routes!(remove_key))
            .routes(routes!(generate_token))
            .routes(routes!(list_token))
            .routes(routes!(remove_token))
            .routes(routes!(list_notification_types))
            .routes(routes!(get_notification_config))
            .routes(routes!(update_notification_config))
            .routes(routes!(get_cla_sign_status))
            .routes(routes!(change_sign_status))
            .routes(routes!(get_cla_content))
            .routes(routes!(update_cla_content))
            .routes(routes!(get_user_approval_status)),
    )
}

async fn user(
    user: LoginUser,
    _: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<LoginUser>>, ApiError> {
    Ok(Json(CommonResult::success(Some(user))))
}

/// Add SSH Key
#[utoipa::path(
    post,
    path = "/ssh",
    request_body = AddSSHKey,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn add_key(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(json): Json<AddSSHKey>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    let ssh_parts: Vec<&str> = json.ssh_key.split_whitespace().collect();
    let key = parse_public_key_base64(
        ssh_parts
            .get(1)
            .ok_or_else(|| MegaError::Other("Invalid key format".to_string()))?,
    )?;
    let title = if json.title.is_empty() {
        ssh_parts
            .get(2)
            .ok_or_else(|| MegaError::Other("Invalid key format".to_string()))?
            .to_string()
    } else {
        json.title
    };
    state
        .services()
        .user()
        .save_ssh_key(
            campsite_user_id,
            &title,
            &json.ssh_key,
            &key.fingerprint(HashAlg::Sha256).to_string(),
        )
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Delete SSH Key
#[utoipa::path(
    delete,
        params(
        ("key_id" = SnowflakeId, Path, description = "A numeric ID representing a SSH"),
    ),
    path = "/ssh/{key_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn remove_key(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(key_id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    state
        .services()
        .user()
        .delete_ssh_key(campsite_user_id, key_id)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Get User's SSH key list
#[utoipa::path(
    get,
    path = "/ssh/list",
    responses(
        (status = 200, body = CommonResult<Vec<ListSSHKey>>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn list_key(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<ListSSHKey>>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    let res = state
        .services()
        .user()
        .list_user_ssh_keys(campsite_user_id)
        .await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Generate Token For http push
#[utoipa::path(
    post,
    path = "/token/generate",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn generate_token(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    let github_login = collaboration_github_login(&user)?.to_string();
    let res = state
        .services()
        .user()
        .generate_user_token(campsite_user_id, Some(github_login))
        .await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Delete User's http push token
#[utoipa::path(
    delete,
        params(
        ("key_id" = SnowflakeId, Path, description = "A numeric ID representing a User Token"),
    ),
    path = "/token/{key_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn remove_token(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(key_id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    state
        .services()
        .user()
        .delete_user_token(campsite_user_id, key_id)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Get User's push token list
#[utoipa::path(
    get,
    path = "/token/list",
    responses(
        (status = 200, body = CommonResult<Vec<ListToken>>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn list_token(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<ListToken>>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?.to_string();
    let data = state
        .services()
        .user()
        .list_user_tokens(campsite_user_id)
        .await?;
    Ok(Json(CommonResult::success(Some(data))))
}

/// List supported notification event types
#[utoipa::path(
    get,
    path = "/notification/types",
    responses((status = 200, body = CommonResult<Vec<NotificationEventTypeInfo>>)),
    tag = USER_TAG
)]
async fn list_notification_types(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<NotificationEventTypeInfo>>>, ApiError> {
    let types = state
        .services()
        .user()
        .list_notification_event_types()
        .await?;

    Ok(Json(CommonResult::success(Some(types))))
}

/// Get current user's notification config
#[utoipa::path(
    get,
    path = "/notification/config",
    responses((status = 200, body = CommonResult<UserNotificationConfig>)),
    tag = USER_TAG
)]
async fn get_notification_config(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<UserNotificationConfig>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?;
    let config = state
        .services()
        .user()
        .get_user_notification_config(campsite_user_id, &user.email)
        .await?;

    Ok(Json(CommonResult::success(Some(config))))
}

/// Update current user's notification config
#[utoipa::path(
    put,
    path = "/notification/config",
    request_body = UpdateUserNotificationConfig,
    responses((status = 200, body = CommonResult<String>)),
    tag = USER_TAG
)]
async fn update_notification_config(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<UpdateUserNotificationConfig>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?;
    state
        .services()
        .user()
        .update_user_notification_config(campsite_user_id, &user.email, payload)
        .await?;

    Ok(Json(CommonResult::success(None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_router_contains_notification_routes() {
        let _router = routers();
    }
}
/// Get current user's CLA sign status
#[utoipa::path(
    get,
    path = "/cla/status",
    responses(
        (status = 200, body = CommonResult<ClaSignStatusRes>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn get_cla_sign_status(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<ClaSignStatusRes>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?;
    let (cla_signed, cla_signed_at) = state
        .services()
        .user()
        .get_or_init_cla_sign_status(campsite_user_id)
        .await?;

    let res = ClaSignStatusRes {
        // API compat: username mirrors campsite_user_id after column drop.
        username: campsite_user_id.to_string(),
        cla_signed,
        cla_signed_at: cla_signed_at.map(|dt| dt.and_utc().timestamp()),
    };
    Ok(Json(CommonResult::success(Some(res))))
}

/// Change CLA sign status for current user
#[utoipa::path(
    post,
    path = "/cla/change-sign-status",
    responses(
        (status = 200, body = CommonResult<ClaSignStatusRes>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn change_sign_status(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<ClaSignStatusRes>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?;
    let (cla_signed, cla_signed_at) = state
        .services()
        .user()
        .change_cla_sign_status(campsite_user_id)
        .await?;

    let res = ClaSignStatusRes {
        username: campsite_user_id.to_string(),
        cla_signed,
        cla_signed_at: cla_signed_at.map(|dt| dt.and_utc().timestamp()),
    };
    Ok(Json(CommonResult::success(Some(res))))
}

/// Get latest CLA text content
#[utoipa::path(
    get,
    path = "/cla/content",
    responses(
        (status = 200, body = CommonResult<ClaContentRes>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn get_cla_content(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<ClaContentRes>>, ApiError> {
    let content = state.services().user().get_cla_content().await?;
    Ok(Json(CommonResult::success(Some(ClaContentRes { content }))))
}

/// Update latest CLA text content
#[utoipa::path(
    post,
    path = "/cla/content",
    request_body = UpdateClaContentPayload,
    responses(
        (status = 200, body = CommonResult<ClaContentRes>, content_type = "application/json")
    ),
    tag = USER_TAG
)]
async fn update_cla_content(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<UpdateClaContentPayload>,
) -> Result<Json<CommonResult<ClaContentRes>>, ApiError> {
    state
        .services()
        .user()
        .update_cla_content(&payload.content)
        .await?;
    Ok(Json(CommonResult::success(Some(ClaContentRes {
        content: payload.content,
    }))))
}

/// Get or initialize current user's account approval status
#[utoipa::path(
    get,
    path = "/approval-status",
    responses(
        (status = 200, body = CommonResult<UserApprovalStatusRes>, content_type = "application/json"),
        (status = 401, description = "Unauthorized"),
    ),
    tag = USER_TAG
)]
async fn get_user_approval_status(
    user: LoginUser,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<UserApprovalStatusRes>>, ApiError> {
    let campsite_user_id = collaboration_actor(&user)?;
    let github_login = user
        .github_login
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let display_name = github_login.unwrap_or(campsite_user_id);
    let model = state
        .services()
        .user()
        .get_or_init_user_approval_status(campsite_user_id, display_name, &user.email, github_login)
        .await?;

    Ok(Json(CommonResult::success(Some(
        state.services().user().to_approval_status_res(model).await,
    ))))
}
