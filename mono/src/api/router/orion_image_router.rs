use anyhow::anyhow;
use api_model::common::CommonResult;
use axum::{
    Json,
    body::Body,
    extract::{DefaultBodyLimit, FromRef, FromRequestParts, Path, Request, State},
    http::{StatusCode, request::Parts},
    routing::put,
};
use ceres::model::orion_image::{
    OrionVmImageListResponse, OrionVmImageResponse, PresignOrionVmImageRequest,
    PresignOrionVmImageResponse, RegisterOrionVmImageRequest,
};
use futures::TryStreamExt;
use io_orbit::object_storage::ObjectByteStream;
use jupiter::{
    service::orion_vm_image_service::OrionVmImageService,
    storage::orion_vm_image_storage::UpsertOrionVmImage,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_common::group_permission::ensure_admin,
    api_doc::ORION_RUNNER_TAG,
    error::ApiError,
    oauth::{BotAuth, api_store::OAuthApiStore, model::LoginUser},
};

/// Max browser-proxied Orion image upload (qcow2).
const ORION_IMAGE_UPLOAD_MAX_BYTES: usize = 32 * 1024 * 1024 * 1024;

/// Bot allowed to register catalog entries via `Authorization: Bearer bot_…`.
const ORION_IMAGE_PUBLISHER_BOT: &str = "orion-image-publisher";

/// Auth for catalog register: admin session cookie, or publisher bot bearer.
struct OrionImageRegisterAuth;

impl<S> FromRequestParts<S> for OrionImageRegisterAuth
where
    MonoApiServiceState: FromRef<S>,
    OAuthApiStore: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let api_state = MonoApiServiceState::from_ref(state);

        if let Ok(bot) = BotAuth::from_request_parts(parts, state).await {
            if bot.bot_name == ORION_IMAGE_PUBLISHER_BOT {
                return Ok(Self);
            }
            tracing::warn!(
                bot_id = bot.bot_id,
                bot_name = %bot.bot_name,
                "orion image register rejected: bot is not {ORION_IMAGE_PUBLISHER_BOT}"
            );
            return Err(ApiError::with_status(
                StatusCode::FORBIDDEN,
                anyhow!("bot is not authorized to register Orion images"),
            ));
        }

        let user = LoginUser::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                ApiError::with_status(StatusCode::UNAUTHORIZED, anyhow!("Unauthorized"))
            })?;
        ensure_admin(&api_state, &user).await?;
        Ok(Self)
    }
}

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/orion/images",
        OpenApiRouter::new()
            .routes(routes!(list_orion_images))
            .routes(routes!(presign_orion_image))
            .routes(routes!(register_orion_image))
            .routes(routes!(delete_orion_image))
            .route(
                "/objects/{*object_key}",
                put(upload_orion_image_object)
                    .layer(DefaultBodyLimit::max(ORION_IMAGE_UPLOAD_MAX_BYTES)),
            ),
    )
}

#[allow(clippy::too_many_arguments)]
fn to_response(
    id: String,
    digest: String,
    object_key: String,
    info_object_key: Option<String>,
    image_name: Option<String>,
    built_at: Option<String>,
    rust: Option<String>,
    buck2: Option<String>,
    python: Option<String>,
    kernel: Option<String>,
    size_bytes: Option<i64>,
    label: Option<String>,
    created_at: chrono::NaiveDateTime,
) -> OrionVmImageResponse {
    OrionVmImageResponse {
        id,
        digest,
        object_key,
        info_object_key,
        image_name,
        built_at,
        rust,
        buck2,
        python,
        kernel,
        size_bytes,
        label,
        created_at: created_at.and_utc().to_rfc3339(),
    }
}

macro_rules! model_to_response {
    ($m:expr) => {
        to_response(
            $m.id,
            $m.digest,
            $m.object_key,
            $m.info_object_key,
            $m.image_name,
            $m.built_at,
            $m.rust,
            $m.buck2,
            $m.python,
            $m.kernel,
            $m.size_bytes,
            $m.label,
            $m.created_at,
        )
    };
}

/// List registered Orion VM images (toolchain metadata for the UI catalog).
#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, body = CommonResult<OrionVmImageListResponse>, content_type = "application/json"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn list_orion_images(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
) -> Result<Json<CommonResult<OrionVmImageListResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let images = state
        .services()
        .storage()
        .orion_vm_image_service
        .list()
        .await
        .map_err(ApiError::from)?;
    let images: Vec<_> = images.into_iter().map(|m| model_to_response!(m)).collect();
    let count = images.len();
    Ok(Json(CommonResult::success(Some(
        OrionVmImageListResponse { count, images },
    ))))
}

/// Prepare catalog object keys for a browser upload (via mono object proxy).
#[utoipa::path(
    post,
    path = "/presign",
    request_body = PresignOrionVmImageRequest,
    responses(
        (status = 200, body = CommonResult<PresignOrionVmImageResponse>, content_type = "application/json"),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn presign_orion_image(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Json(req): Json<PresignOrionVmImageRequest>,
) -> Result<Json<CommonResult<PresignOrionVmImageResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let digest = req.digest.trim().to_string();
    if digest.is_empty() {
        return Err(ApiError::bad_request(anyhow!("digest is required")));
    }
    if !(digest.starts_with("sha256:") || digest.starts_with("sha512:")) {
        return Err(ApiError::bad_request(anyhow!(
            "digest must start with sha256: or sha512:"
        )));
    }

    let image_name = req
        .image_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("debian-13-buck2");
    let (object_key, info_object_key) = OrionVmImageService::object_keys(&digest, image_name);

    // Browser uploads go through mono (`PUT /objects/{key}`) so clients do not need
    // a browser-reachable RustFS endpoint / CORS. Paths are relative to the mono API root.
    let qcow2_put_url = format!("/api/v1/orion/images/objects/{object_key}");
    let (info_object_key, info_put_url) = if req.with_info {
        (
            Some(info_object_key.clone()),
            Some(format!("/api/v1/orion/images/objects/{info_object_key}")),
        )
    } else {
        (None, None)
    };

    Ok(Json(CommonResult::success(Some(
        PresignOrionVmImageResponse {
            object_key,
            info_object_key,
            qcow2_put_url,
            info_put_url,
            expires_in_secs: 0,
        },
    ))))
}

/// Stream object bytes into the Orion image namespace (admin browser upload proxy).
async fn upload_orion_image_object(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(object_key): Path<String>,
    req: Request<Body>,
) -> Result<StatusCode, ApiError> {
    ensure_admin(&state, &user).await?;
    let object_key = object_key.trim().trim_start_matches('/').to_string();
    if object_key.is_empty()
        || object_key.contains("..")
        || object_key.starts_with('/')
        || !object_key.contains('/')
    {
        return Err(ApiError::bad_request(anyhow!(
            "object_key must look like '{{digest_hex}}/{{filename}}'"
        )));
    }

    let data: ObjectByteStream = Box::pin(
        req.into_body()
            .into_data_stream()
            .map_err(std::io::Error::other),
    );

    state
        .services()
        .storage()
        .orion_vm_image_service
        .put_object_stream(&object_key, data)
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Register (upsert) an image after build-script upload to RustFS.
#[utoipa::path(
    post,
    path = "/",
    request_body = RegisterOrionVmImageRequest,
    responses(
        (status = 200, body = CommonResult<OrionVmImageResponse>, content_type = "application/json"),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin session or orion-image-publisher bot"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn register_orion_image(
    _auth: OrionImageRegisterAuth,
    State(state): State<MonoApiServiceState>,
    Json(req): Json<RegisterOrionVmImageRequest>,
) -> Result<Json<CommonResult<OrionVmImageResponse>>, ApiError> {
    let digest = req.digest.trim().to_string();
    if digest.is_empty() || req.object_key.trim().is_empty() {
        return Err(ApiError::bad_request(anyhow!(
            "digest and object_key are required"
        )));
    }
    if !(digest.starts_with("sha256:") || digest.starts_with("sha512:")) {
        return Err(ApiError::bad_request(anyhow!(
            "digest must start with sha256: or sha512:"
        )));
    }

    let model = state
        .services()
        .storage()
        .orion_vm_image_service
        .upsert(UpsertOrionVmImage {
            digest,
            object_key: req.object_key.trim().trim_start_matches('/').to_string(),
            info_object_key: req
                .info_object_key
                .map(|k| k.trim().trim_start_matches('/').to_string())
                .filter(|k| !k.is_empty()),
            image_name: req.image_name,
            built_at: req.built_at,
            rust: req.rust,
            buck2: req.buck2,
            python: req.python,
            kernel: req.kernel,
            size_bytes: req.size_bytes,
            label: req.label,
        })
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CommonResult::success(Some(model_to_response!(model)))))
}

/// Delete a catalog entry and its RustFS objects.
#[utoipa::path(
    delete,
    path = "/{id}",
    params(
        ("id" = String, Path, description = "Catalog image id")
    ),
    responses(
        (status = 200, body = CommonResult<OrionVmImageResponse>, content_type = "application/json"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 404, description = "Not found"),
        (status = 409, description = "Image still in use by a runner"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn delete_orion_image(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(id): Path<String>,
) -> Result<Json<CommonResult<OrionVmImageResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let svc = &state.services().storage().orion_vm_image_service;
    let existing =
        svc.get(&id).await.map_err(ApiError::from)?.ok_or_else(|| {
            ApiError::with_status(StatusCode::NOT_FOUND, anyhow!("image not found"))
        })?;

    if let Some(client) = state.orion_scheduler_client()
        && let Ok(list) = client.list_vms().await
    {
        let in_use = list.vms.iter().any(|vm| {
            vm.image_digest
                .as_deref()
                .is_some_and(|d| d == existing.digest)
        });
        if in_use {
            return Err(ApiError::with_status(
                StatusCode::CONFLICT,
                anyhow!(
                    "image {} is still referenced by a tracked runner",
                    existing.digest
                ),
            ));
        }
    }

    let deleted = svc
        .delete(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::with_status(StatusCode::NOT_FOUND, anyhow!("image not found")))?;

    Ok(Json(CommonResult::success(Some(model_to_response!(
        deleted
    )))))
}
