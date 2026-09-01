use api_model::common::{CommonPage, CommonResult, Pagination};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use ceres::model::{
    serde_snowflake::SnowflakeId,
    webhook::{CreateWebhookRequest, ListWebhooksQuery, WebhookResponse},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{MonoApiServiceState, api_doc::WEBHOOK_TAG, error::ApiError};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new()
        .routes(routes!(create_webhook))
        .routes(routes!(list_webhooks))
        .routes(routes!(delete_webhook))
}

/// Create a webhook
#[utoipa::path(
    post,
    path = "/webhooks",
    request_body = CreateWebhookRequest,
    responses(
        (status = 200, body = CommonResult<WebhookResponse>, content_type = "application/json")
    ),
    tag = WEBHOOK_TAG
)]
async fn create_webhook(
    state: State<MonoApiServiceState>,
    Json(payload): Json<CreateWebhookRequest>,
) -> Result<Json<CommonResult<WebhookResponse>>, ApiError> {
    let created = state.services().webhook().create_webhook(payload).await?;
    Ok(Json(CommonResult::success(Some(created))))
}

/// List webhooks
#[utoipa::path(
    get,
    path = "/webhooks",
    params(
        ("page" = Option<u64>, Query, description = "Page number, starts from 1. Default: 1"),
        ("per_page" = Option<u64>, Query, description = "Items per page. Default: 20")
    ),
    responses(
        (status = 200, body = CommonResult<CommonPage<WebhookResponse>>, content_type = "application/json")
    ),
    tag = WEBHOOK_TAG
)]
async fn list_webhooks(
    state: State<MonoApiServiceState>,
    Query(query): Query<ListWebhooksQuery>,
) -> Result<Json<CommonResult<CommonPage<WebhookResponse>>>, ApiError> {
    let pagination = build_webhook_pagination(query)?;
    let (items, total) = state.services().webhook().list_webhooks(pagination).await?;
    Ok(Json(CommonResult::success(Some(CommonPage {
        total,
        items,
    }))))
}

/// Delete a webhook
#[utoipa::path(
    delete,
    params(("id" = SnowflakeId, Path, description = "Webhook ID")),
    path = "/webhooks/{id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json"),
        (status = 404, description = "Webhook not found")
    ),
    tag = WEBHOOK_TAG
)]
async fn delete_webhook(
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state.services().webhook().delete_webhook(id).await?;
    Ok(Json(CommonResult::success(None)))
}

fn build_webhook_pagination(query: ListWebhooksQuery) -> Result<Pagination, ApiError> {
    let mut pagination = Pagination::default();
    if let Some(page) = query.page {
        pagination.page = page;
    }
    if let Some(per_page) = query.per_page {
        pagination.per_page = per_page;
    }

    if pagination.page == 0 {
        return Err(ApiError::bad_request(anyhow::anyhow!(
            "page must be greater than or equal to 1"
        )));
    }
    if pagination.per_page == 0 {
        return Err(ApiError::bad_request(anyhow::anyhow!(
            "per_page must be greater than or equal to 1"
        )));
    }

    Ok(pagination)
}
