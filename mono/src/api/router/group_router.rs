use anyhow::anyhow;
use api_model::common::{CommonPage, CommonResult, PageParams};
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::{
    group::{
        AddMembersRequest, CreateGroupRequest, DeleteGroupResponse, DeletePermissionsResponse,
        EmptyListAdditional, GroupMemberResponse, GroupResponse, RemoveMemberResponse,
        ResourcePermissionResponse, SetPermissionsRequest, UpdateGroupRequest,
        UserEffectivePermissionResponse, UserGroupsResponse,
    },
    serde_snowflake::SnowflakeId,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_common::group_permission::{
        build_user_effective_permission_response, ensure_admin, resolve_resource_context,
    },
    api_doc::GROUP_PERMISSION_TAG,
    error::ApiError,
    oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/admin",
        OpenApiRouter::new()
            .routes(routes!(create_group))
            .routes(routes!(list_groups))
            .routes(routes!(get_group))
            .routes(routes!(update_group))
            .routes(routes!(delete_group))
            .routes(routes!(add_group_members))
            .routes(routes!(remove_group_member))
            .routes(routes!(list_group_members))
            .routes(routes!(set_resource_permissions))
            .routes(routes!(get_resource_permissions))
            .routes(routes!(update_resource_permissions))
            .routes(routes!(delete_resource_permissions))
            .routes(routes!(get_user_groups))
            .routes(routes!(get_user_effective_permission)),
    )
}

#[utoipa::path(
    post,
    path = "/groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 200, body = CommonResult<GroupResponse>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 409, description = "Group already exists"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn create_group(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<CommonResult<GroupResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let group = state.services().admin().create_group(req).await?;

    Ok(Json(CommonResult::success(Some(group.into()))))
}

#[utoipa::path(
    post,
    path = "/groups/list",
    request_body = PageParams<EmptyListAdditional>,
    responses(
        (status = 200, body = CommonResult<CommonPage<GroupResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn list_groups(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Json(json): Json<PageParams<EmptyListAdditional>>,
) -> Result<Json<CommonResult<CommonPage<GroupResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let (items, total) = state
        .services()
        .admin()
        .list_groups(json.pagination)
        .await?;
    let items = items.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(CommonPage {
        total,
        items,
    }))))
}

#[utoipa::path(
    get,
    path = "/groups/{group_id}",
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID")
    ),
    responses(
        (status = 200, body = CommonResult<GroupResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn get_group(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(SnowflakeId(group_id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<GroupResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let group = state
        .services()
        .admin()
        .get_group_by_id(group_id)
        .await?
        .ok_or_else(|| {
            tracing::warn!(
                campsite_user_id = %user.campsite_user_id,
                group_id,
                "group.get failed: group not found"
            );
            ApiError::not_found(anyhow!("Group not found: {}", group_id))
        })?;

    Ok(Json(CommonResult::success(Some(group.into()))))
}

#[utoipa::path(
    put,
    path = "/groups/{group_id}",
    request_body = UpdateGroupRequest,
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID")
    ),
    responses(
        (status = 200, body = CommonResult<GroupResponse>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
        (status = 409, description = "Group already exists"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn update_group(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(SnowflakeId(group_id)): Path<SnowflakeId>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<Json<CommonResult<GroupResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let updated = state
        .services()
        .admin()
        .update_group(
            group_id,
            UpdateGroupRequest {
                name: req.name,
                description: req.description,
            },
        )
        .await?;

    Ok(Json(CommonResult::success(Some(updated.into()))))
}

#[utoipa::path(
    delete,
    path = "/groups/{group_id}",
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID")
    ),
    responses(
        (status = 200, body = CommonResult<DeleteGroupResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn delete_group(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(SnowflakeId(group_id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<DeleteGroupResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let stats = state.services().admin().delete_group(group_id).await?;

    Ok(Json(CommonResult::success(Some(DeleteGroupResponse {
        group_id,
        deleted_members_count: stats.deleted_members_count,
        deleted_permissions_count: stats.deleted_permissions_count,
        deleted_groups_count: stats.deleted_groups_count,
    }))))
}

#[utoipa::path(
    post,
    path = "/groups/{group_id}/members",
    request_body = AddMembersRequest,
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID")
    ),
    responses(
        (status = 200, body = CommonResult<Vec<GroupMemberResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn add_group_members(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(SnowflakeId(group_id)): Path<SnowflakeId>,
    Json(req): Json<AddMembersRequest>,
) -> Result<Json<CommonResult<Vec<GroupMemberResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let members = state
        .services()
        .admin()
        .add_group_members(group_id, req.usernames)
        .await?;
    let members = members.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(members))))
}

#[utoipa::path(
    delete,
    path = "/groups/{group_id}/members/{username}",
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID"),
        ("username" = String, Path, description = "Campsite user id of the member")
    ),
    responses(
        (status = 200, body = CommonResult<RemoveMemberResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn remove_group_member(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((group_id, username)): Path<(SnowflakeId, String)>,
) -> Result<Json<CommonResult<RemoveMemberResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    // Path `username` is campsite_user_id.
    let removed = state
        .services()
        .admin()
        .remove_group_member(group_id.into(), &username)
        .await?;

    Ok(Json(CommonResult::success(Some(RemoveMemberResponse {
        group_id: group_id.into(),
        username,
        removed,
    }))))
}

#[utoipa::path(
    post,
    path = "/groups/{group_id}/members/list",
    request_body = PageParams<EmptyListAdditional>,
    params(
        ("group_id" = SnowflakeId, Path, description = "Group ID")
    ),
    responses(
        (status = 200, body = CommonResult<CommonPage<GroupMemberResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn list_group_members(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(SnowflakeId(group_id)): Path<SnowflakeId>,
    Json(json): Json<PageParams<EmptyListAdditional>>,
) -> Result<Json<CommonResult<CommonPage<GroupMemberResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let (items, total) = state
        .services()
        .admin()
        .list_group_members(group_id, json.pagination)
        .await?;
    let items = items.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(CommonPage {
        total,
        items,
    }))))
}

#[utoipa::path(
    post,
    path = "/resources/{resource_type}/{resource_id}/permissions",
    request_body = SetPermissionsRequest,
    params(
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<Vec<ResourcePermissionResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Resource or group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn set_resource_permissions(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((resource_type, resource_id)): Path<(String, String)>,
    Json(req): Json<SetPermissionsRequest>,
) -> Result<Json<CommonResult<Vec<ResourcePermissionResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let (resource_type_value, resource_id) =
        resolve_resource_context(&state, resource_type.as_str(), &resource_id).await?;

    let saved = state
        .services()
        .admin()
        .set_resource_permission(resource_type_value.into(), &resource_id, req.permissions)
        .await?;
    let saved = saved.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(saved))))
}

#[utoipa::path(
    get,
    path = "/resources/{resource_type}/{resource_id}/permissions",
    params(
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<Vec<ResourcePermissionResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Resource not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn get_resource_permissions(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((resource_type, resource_id)): Path<(String, String)>,
) -> Result<Json<CommonResult<Vec<ResourcePermissionResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let (resource_type_value, resource_id) =
        resolve_resource_context(&state, resource_type.as_str(), &resource_id).await?;

    let permissions = state
        .services()
        .admin()
        .get_resource_permissions(resource_type_value.into(), &resource_id)
        .await?;
    let permissions = permissions.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(permissions))))
}

#[utoipa::path(
    put,
    path = "/resources/{resource_type}/{resource_id}/permissions",
    request_body = SetPermissionsRequest,
    params(
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<Vec<ResourcePermissionResponse>>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Resource or group not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn update_resource_permissions(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((resource_type, resource_id)): Path<(String, String)>,
    Json(req): Json<SetPermissionsRequest>,
) -> Result<Json<CommonResult<Vec<ResourcePermissionResponse>>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let (resource_type_value, resource_id) =
        resolve_resource_context(&state, resource_type.as_str(), &resource_id).await?;

    let updated = state
        .services()
        .admin()
        .update_resource_permissions(resource_type_value.into(), &resource_id, req.permissions)
        .await?;
    let updated = updated.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(updated))))
}

#[utoipa::path(
    delete,
    path = "/resources/{resource_type}/{resource_id}/permissions",
    params(
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<DeletePermissionsResponse>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Resource not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn delete_resource_permissions(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((resource_type, resource_id)): Path<(String, String)>,
) -> Result<Json<CommonResult<DeletePermissionsResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let (resource_type_value, resource_id) =
        resolve_resource_context(&state, resource_type.as_str(), &resource_id).await?;

    let deleted_count = state
        .services()
        .admin()
        .delete_resource_permissions(resource_type_value.into(), &resource_id)
        .await?;

    Ok(Json(CommonResult::success(Some(
        DeletePermissionsResponse {
            resource_type: resource_type_value,
            resource_id,
            deleted_count,
        },
    ))))
}

#[utoipa::path(
    get,
    path = "/users/{username}/groups",
    params(
        ("username" = String, Path, description = "Campsite user id")
    ),
    responses(
        (status = 200, body = CommonResult<UserGroupsResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn get_user_groups(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(username): Path<String>,
) -> Result<Json<CommonResult<UserGroupsResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    // Path `username` is campsite_user_id.
    let groups = state.services().admin().get_user_groups(&username).await?;
    let groups = groups.into_iter().map(Into::into).collect();

    Ok(Json(CommonResult::success(Some(UserGroupsResponse {
        username,
        groups,
    }))))
}

#[utoipa::path(
    get,
    path = "/users/{username}/permissions/{resource_type}/{resource_id}",
    params(
        ("username" = String, Path, description = "Campsite user id"),
        ("resource_type" = String, Path, description = "Resource type, currently only `note`"),
        ("resource_id" = String, Path, description = "Resource ID")
    ),
    responses(
        (status = 200, body = CommonResult<UserEffectivePermissionResponse>),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Resource not found"),
    ),
    tag = GROUP_PERMISSION_TAG
)]
async fn get_user_effective_permission(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path((username, resource_type, resource_id)): Path<(String, String, String)>,
) -> Result<Json<CommonResult<UserEffectivePermissionResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let (resource_type_value, resource_id) =
        resolve_resource_context(&state, resource_type.as_str(), &resource_id).await?;

    // Path `username` is campsite_user_id.
    let effective = state
        .services()
        .admin()
        .get_user_effective_permission(&username, resource_type_value.into(), &resource_id)
        .await?;
    let response = build_user_effective_permission_response(
        username,
        resource_type_value,
        resource_id,
        effective,
    );

    Ok(Json(CommonResult::success(Some(response))))
}
