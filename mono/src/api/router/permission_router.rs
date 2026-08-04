use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::group::UserEffectivePermissionResponse;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_common::{
        group_permission::{build_user_effective_permission_response, resolve_resource_context},
        identity::collaboration_actor,
    },
    api_doc::GROUP_PERMISSION_TAG,
    error::ApiError,
    oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/permissions",
        OpenApiRouter::new().routes(routes!(get_my_permission)),
    )
}

#[utoipa::path(
    get,
    path = "/me/{resource_type}/{resource_id}",
    params(
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<UserEffectivePermissionResponse>),
        (status = 400, description = "Invalid resource_type or resource_id"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Resource not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn get_my_permission(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((resource_type, resource_id)): Path<(String, String)>,
) -> Result<Json<CommonResult<UserEffectivePermissionResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?.to_string();

    let (resource_type_value, normalized_id) =
        resolve_resource_context(&state, &resource_type, &resource_id).await?;

    let effective = state
        .services()
        .admin()
        .get_user_effective_permission(&actor, resource_type_value.into(), &normalized_id)
        .await?;

    let response = build_user_effective_permission_response(
        actor,
        resource_type_value,
        normalized_id,
        effective,
    );

    Ok(Json(CommonResult::success(Some(response))))
}
