use api_model::common::{CommonPage, CommonResult, PageParams};
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::{
    label::{LabelItem, NewLabel},
    serde_snowflake::SnowflakeId,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_doc::LABEL_TAG, error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/label",
        OpenApiRouter::new()
            .routes(routes!(new_label))
            .routes(routes!(fetch_label_list))
            .routes(routes!(fetch_label)),
    )
}

/// List label in page
#[utoipa::path(
    post,
    path = "/list",
    request_body = PageParams<String>,
    responses(
        (status = 200, body = CommonResult<CommonPage<LabelItem>>, content_type = "application/json")
    ),
    tag = LABEL_TAG
)]
async fn fetch_label_list(
    state: State<MonoApiServiceState>,
    Json(json): Json<PageParams<String>>,
) -> Result<Json<CommonResult<CommonPage<LabelItem>>>, ApiError> {
    let (items, total) = state
        .services()
        .issue()
        .list_labels_by_page(json.pagination, &json.additional)
        .await?;
    Ok(Json(CommonResult::success(Some(CommonPage {
        items,
        total,
    }))))
}

/// New label
#[utoipa::path(
    post,
    path = "/new",
    request_body = NewLabel,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = LABEL_TAG
)]
async fn new_label(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(json): Json<NewLabel>,
) -> Result<Json<CommonResult<LabelItem>>, ApiError> {
    let res = state
        .services()
        .issue()
        .new_label(&json.name, &json.color, &json.description)
        .await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Fetch label details
#[utoipa::path(
    get,
        params(
        ("id" = SnowflakeId, Path, description = "Label's id"),
    ),
    path = "/{id}",
    responses(
        (status = 200, body = CommonResult<LabelItem>, content_type = "application/json")
    ),
    tag = LABEL_TAG
)]
async fn fetch_label(
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<LabelItem>>, ApiError> {
    let label = state.services().issue().get_label_by_id(id).await?;
    Ok(Json(CommonResult::success(label)))
}
