use api_model::common::{CommonPage, CommonResult, PageParams};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use ceres::model::{
    change_list::{AssigneeUpdatePayload, ListPayload},
    conversation::{ContentPayload, ConvType},
    issue::{IssueDetailRes, IssueSuggestions, ItemRes, NewIssue, QueryPayload},
    label::LabelUpdatePayload,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common, api_common::identity::collaboration_actor, api_doc::ISSUE_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/issue",
        OpenApiRouter::new()
            .routes(routes!(fetch_issue_list))
            .routes(routes!(new_issue))
            .routes(routes!(close_issue))
            .routes(routes!(reopen_issue))
            .routes(routes!(issue_detail))
            .routes(routes!(save_comment))
            .routes(routes!(labels))
            .routes(routes!(assignees))
            .routes(routes!(edit_title))
            .routes(routes!(issue_suggester)),
    )
}

/// Fetch Issue list
#[utoipa::path(
    post,
    path = "/list",
    request_body = PageParams<ListPayload>,
    responses(
        (status = 200, body = CommonResult<CommonPage<ItemRes>>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn fetch_issue_list(
    state: State<MonoApiServiceState>,
    Json(json): Json<PageParams<ListPayload>>,
) -> Result<Json<CommonResult<CommonPage<ItemRes>>>, ApiError> {
    let (items, total) = state
        .services()
        .issue()
        .get_issue_list(json.additional, json.pagination)
        .await?;
    Ok(Json(CommonResult::success(Some(CommonPage {
        items,
        total,
    }))))
}

/// Get issue details
#[utoipa::path(
    get,
    params(
        ("link", description = "Issue link"),
    ),
    path = "/{link}/detail",
    responses(
        (status = 200, body = CommonResult<IssueDetailRes>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn issue_detail(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<IssueDetailRes>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let issue_details = state
        .services()
        .issue()
        .get_issue_details(&link, actor.to_string())
        .await?;
    Ok(Json(CommonResult::success(Some(issue_details))))
}

/// New Issue
#[utoipa::path(
    post,
    path = "/new",
    request_body = NewIssue,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn new_issue(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(json): Json<NewIssue>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let res = state
        .services()
        .issue()
        .save_issue(actor, &json.title)
        .await?;
    state
        .services()
        .conversation()
        .add_conversation(&res.link, actor, Some(json.description), ConvType::Comment)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Close an issue
#[utoipa::path(
    post,
    params(
        ("link", description = "Issue link"),
    ),
    path = "/{link}/close",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn close_issue(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state.services().issue().close_issue(&link).await?;
    state
        .services()
        .conversation()
        .add_system_event(&link, actor, "closed this", ConvType::Closed)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Reopen an issue
#[utoipa::path(
    post,
    params(
        ("link", description = "Issue link"),
    ),
    path = "/{link}/reopen",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn reopen_issue(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state.services().issue().reopen_issue(&link).await?;
    state
        .services()
        .conversation()
        .add_system_event(&link, actor, "reopen this", ConvType::Reopen)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Add new comment on Issue
#[utoipa::path(
    post,
    params(
        ("link", description = "Issue link"),
    ),
    path = "/{link}/comment",
    request_body = ContentPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn save_comment(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ContentPayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .conversation()
        .add_conversation(
            &link,
            actor,
            Some(payload.content.clone()),
            ConvType::Comment,
        )
        .await?;
    api_common::comment::check_comment_ref(user, state, &payload.content, &link).await
}

/// Update issue related labels
#[utoipa::path(
    post,
    path = "/labels",
    request_body = LabelUpdatePayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn labels(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<LabelUpdatePayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    api_common::label_assignee::label_update(user, state, payload, String::from("issue")).await
}

/// Update issue related assignees
#[utoipa::path(
    post,
    path = "/assignees",
    request_body = AssigneeUpdatePayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn assignees(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<AssigneeUpdatePayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    api_common::label_assignee::assignees_update(user, state, payload, String::from("issue")).await
}

/// Edit issue title
#[utoipa::path(
    post,
    params(
        ("link", description = "A string ID representing a Issue"),
    ),
    path = "/{link}/title",
    request_body = ContentPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn edit_title(
    _: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ContentPayload>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .issue()
        .edit_issue_title(&link, &payload.content)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Get issue suggester in comment
#[utoipa::path(
    get,
    params(QueryPayload),
    path = "/issue_suggester",
    responses(
        (status = 200, body = CommonResult<Vec<IssueSuggestions>>, content_type = "application/json")
    ),
    tag = ISSUE_TAG
)]
async fn issue_suggester(
    Query(payload): Query<QueryPayload>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<IssueSuggestions>>>, ApiError> {
    let res = state
        .services()
        .issue()
        .get_issue_suggestions(&payload.query)
        .await?;
    Ok(Json(CommonResult::success(Some(res))))
}
