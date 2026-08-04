use api_model::common::CommonResult;
use axum::{Json, extract::State};
use ceres::model::{change_list::AssigneeUpdatePayload, label::LabelUpdatePayload};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, error::ApiError,
    oauth::model::LoginUser,
};

pub async fn label_update(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    payload: LabelUpdatePayload,
    item_type: String,
) -> Result<Json<CommonResult<()>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let LabelUpdatePayload {
        label_ids,
        link,
        item_id,
    } = payload;

    state
        .services()
        .issue()
        .update_item_labels(actor, item_id, &item_type, label_ids, &link)
        .await?;

    Ok(Json(CommonResult::success(None)))
}

pub async fn assignees_update(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    payload: AssigneeUpdatePayload,
    item_type: String,
) -> Result<Json<CommonResult<()>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let AssigneeUpdatePayload {
        assignees,
        link,
        item_id,
    } = payload;

    state
        .services()
        .issue()
        .update_item_assignees(actor, item_id, &item_type, assignees, &link)
        .await?;

    Ok(Json(CommonResult::success(None)))
}
