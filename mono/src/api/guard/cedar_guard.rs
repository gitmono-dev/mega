use std::{collections::HashMap, str::FromStr};

use axum::{
    extract::{FromRef, FromRequestParts, Request, State},
    middleware::Next,
    response::Response,
};
use cedar_policy::{Context, EntityId, EntityTypeName, EntityUid};
use common::errors::MegaError;
use once_cell::sync::Lazy;
use saturn::{ActionEnum, context::CedarContext, entitystore::EntityStore, util::SaturnEUid};

use crate::api::{
    MonoApiServiceState,
    error::ApiError,
    oauth::{BotAuth, model::LoginUser},
};

// TODO: All users are temporary allowed during development stage
const POLICY_CONTENT: &str = r#"
permit (
    principal,  
    action,
    resource
);"#;

type EndPointConfig = HashMap<String, HashMap<String, String>>;
static GURADED_ENDPOINTS: Lazy<EndPointConfig> = Lazy::new(|| {
    let endpoints_config_dict: &str = include_str!("guarded_endpoints.json");
    serde_json::from_str(endpoints_config_dict).unwrap_or_else(|e| {
        tracing::error!("Failed to read endpoints configuration for guard {:}", e);
        EndPointConfig::new()
    })
});

pub fn resolve_cl_action(req_path: &str) -> Result<(ActionEnum, String), MegaError> {
    let cl_path_prefix = "/cl";
    // Avoid parsing request of non-CL endpoints
    if !req_path.starts_with(cl_path_prefix) {
        return Ok((ActionEnum::UnprotectedRequest, String::new()));
    }
    let path = req_path.trim_start_matches(cl_path_prefix);

    let cl_config = GURADED_ENDPOINTS.get(cl_path_prefix).ok_or_else(|| {
        MegaError::Other("No CL config found in guarded_endpoints.json".to_string())
    })?;

    let Some((action, mr_link)) = match_operation(path, cl_config) else {
        tracing::warn!("No matching CL action for path: {}", req_path);
        return Ok((ActionEnum::UnprotectedRequest, String::new()));
    };

    Ok((action, mr_link))
}

//TODO: Only match cl api paths for now, extend when need in the future
/// return (ActionEnum, mr_link)
fn match_operation(
    suffix: &str,
    patterns: &HashMap<String, String>,
) -> Option<(ActionEnum, String)> {
    let suffix = suffix.trim_matches('/');

    for (pattern, action) in patterns {
        let pattern_trimmed = pattern.trim_matches('/');

        if pattern_trimmed.contains("{link}") {
            let parts: Vec<&str> = pattern_trimmed.split("{link}").collect();
            if parts.len() == 2 {
                let prefix = parts[0].trim_matches('/');
                let op = parts[1].trim_matches('/');

                if (prefix.is_empty() || suffix.starts_with(prefix))
                    && (op.is_empty() || suffix.ends_with(op))
                {
                    // Bounds check: ensure suffix is long enough
                    let prefix_len = prefix.len();
                    let op_len = op.len();
                    if prefix_len + op_len > suffix.len() {
                        continue;
                    }

                    let start = if prefix.is_empty() { 0 } else { prefix_len };
                    let end = if op.is_empty() {
                        suffix.len()
                    } else {
                        suffix.len() - op_len
                    };

                    if start > end {
                        continue;
                    }

                    let mr_link = &suffix[start..end];

                    return Some((
                        ActionEnum::from(action.as_str()),
                        mr_link.trim_matches('/').to_string(),
                    ));
                }
            }
        } else if suffix == pattern_trimmed {
            return Some((ActionEnum::from(action.as_str()), String::new()));
        }
    }
    None
}

pub async fn cedar_guard(
    State(state): State<MonoApiServiceState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let request_path = req.uri().path().to_owned();
    tracing::debug!("Processing request: {}", request_path);

    let (action, link) = resolve_cl_action(&request_path).map_err(|e| {
        tracing::error!("Failed to resolve CL action: {}", e);
        ApiError::from(e)
    })?;
    tracing::debug!("Resolved action: {:?}, link: {}", action, link);

    // Skip authorization for unprotected requests
    if action.eq(&ActionEnum::UnprotectedRequest) {
        tracing::debug!("Unprotected request for path: {}", request_path);
        return Ok(next.run(req).await);
    }

    // TODO: Fetch repo path from CL model
    // let cl_model = state
    //     .cl_stg()
    //     .get_cl(&link)
    //     .await?
    //     .ok_or_else(|| MegaError::with_message(format!("Change list not found for link: {}", link)))?;
    // let repo_path: PathBuf = cl_model.path.into();

    let (mut parts, body) = req.into_parts();

    let (principal_type, principal_id) =
        if let Ok(bot) = BotAuth::from_request_parts(&mut parts, &state).await {
            ("Bot".to_string(), bot.bot_id.to_string())
        } else if let Ok(user) = LoginUser::from_request_parts(&mut parts, &state).await {
            ("User".to_string(), user.cedar_user_id().to_string())
        } else {
            ("User".to_string(), "reader".to_string())
        };

    // let policy_path = repo_path.join("cedar/policies.cedar");
    // let policy_content = get_blob_string(&state, &policy_path).await?;
    let policy_content = POLICY_CONTENT.to_string();

    let entity_store = EntityStore::from_ref(&state);
    let c = CedarContext::from(entity_store, &policy_content)?;

    authorize(&c, &principal_type, &principal_id, &action.to_string())
        .await
        .map_err(|e| {
            tracing::debug!(
                "Authorization failed for {}: {}",
                &principal_id,
                &action.to_string()
            );
            ApiError::from(MegaError::forbidden(format!(
                "Guard Authorization failed: {e}"
            )))
        })?;

    let req = Request::from_parts(parts, body);
    let response = next.run(req).await;

    if response.status().is_client_error() {
        tracing::warn!(
            status = %response.status(),
            path = %request_path,
            "Downstream returned a 4xx error"
        );
    }

    Ok(response)
}

async fn authorize(
    cedar_context: &CedarContext,
    principal_type: &str,
    principal_id: &str,
    action: &str,
) -> Result<(), MegaError> {
    let user_entity = EntityId::from_str(principal_id)?;
    let action_entity = EntityId::from_str(action)?;

    let role_entity = EntityTypeName::from_str(principal_type)?;
    let actiontype_entity = EntityTypeName::from_str("Action")?;

    let principal = SaturnEUid::from(EntityUid::from_type_name_and_id(role_entity, user_entity));
    let action = SaturnEUid::from(EntityUid::from_type_name_and_id(
        actiontype_entity,
        action_entity,
    ));
    // TODO: repository is currently hardcoded to "0", need to change it to actual repo id
    let resource = SaturnEUid::from(EntityUid::from_type_name_and_id(
        EntityTypeName::from_str("Repository")?,
        EntityId::from_str("0")?,
    ));

    let context = Context::empty();

    cedar_context
        .is_authorized(&principal, &action, &resource, context)
        .map_err(|e| MegaError::forbidden(format!("Authorization failed: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_match_operation() {
        let patterns: HashMap<String, String> = HashMap::from([
            (
                "/{link}/approve".to_string(),
                "approveChangeList".to_string(),
            ),
            ("/{link}/close".to_string(), "editChangeList".to_string()),
            (
                "/{link}/review/delete".to_string(),
                "editChangeList".to_string(),
            ),
        ]);

        let suffix = "/my-cl-link/approve";
        let result = match_operation(suffix, &patterns);
        assert_eq!(
            result,
            Some((ActionEnum::ApproveChangeList, "my-cl-link".to_string()))
        );

        let suffix = "/another-cl-link/close";
        let result = match_operation(suffix, &patterns);
        assert_eq!(
            result,
            Some((ActionEnum::EditChangeList, "another-cl-link".to_string()))
        );

        let suffix = "/no-match-link/delete";
        let result = match_operation(suffix, &patterns);
        assert_eq!(result, None);

        let suffix = "/path/subpath/review/delete";
        let result = match_operation(suffix, &patterns);
        assert_eq!(
            result,
            Some((ActionEnum::EditChangeList, "path/subpath".to_string()))
        );
    }
}
