use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use common::errors::MegaError;

use super::changes_calculator::MonoChangesCalculator;
use crate::application::build_trigger::{
    BuildTrigger, BuildTriggerPayload, BuildTriggerType, ChangesPort, TriggerContext,
    TriggerHandler, WebEditPayload,
};

fn fallback_cl_link(commit_hash: &str, now_millis: i64) -> String {
    format!(
        "webedit-{now_millis}-{}",
        &commit_hash[..8.min(commit_hash.len())]
    )
}

fn resolve_cl_link(context: &TriggerContext, now_millis: i64) -> String {
    context
        .cl_link
        .clone()
        .unwrap_or_else(|| fallback_cl_link(&context.commit_hash, now_millis))
}

fn serialize_builds(
    builds: &[api_model::buck2::status::Status<api_model::buck2::types::ProjectRelativePath>],
) -> Result<serde_json::Value, MegaError> {
    serde_json::to_value(builds)
        .map_err(|e| MegaError::Other(format!("Failed to serialize builds: {}", e)))
}

/// Handler for web edit trigger
pub struct WebEditHandler {
    changes_calculator: MonoChangesCalculator,
}

impl WebEditHandler {
    pub fn new(changes_port: Arc<dyn ChangesPort>) -> Self {
        Self {
            changes_calculator: MonoChangesCalculator::new(changes_port),
        }
    }
}

#[async_trait]
impl TriggerHandler for WebEditHandler {
    async fn handle(&self, context: &TriggerContext) -> Result<BuildTrigger, MegaError> {
        let builds = self
            .changes_calculator
            .get_builds_for_commit(context)
            .await?;

        let now = Utc::now();
        let cl_link = resolve_cl_link(context, now.timestamp_millis());

        Ok(BuildTrigger {
            trigger_type: context.trigger_type,
            trigger_source: context.trigger_source,
            trigger_time: now,
            payload: BuildTriggerPayload::WebEdit(WebEditPayload {
                repo: context.repo_path.clone(),
                from_hash: context.from_hash.clone(),
                commit_hash: context.commit_hash.clone(),
                cl_link,
                cl_id: context.cl_id,
                builds: serialize_builds(&builds)?,
                triggered_by: context.triggered_by.clone(),
            }),
        })
    }

    fn trigger_type(&self) -> BuildTriggerType {
        BuildTriggerType::WebEdit
    }
}

#[cfg(test)]
mod tests {
    use api_model::buck2::{status::Status, types::ProjectRelativePath};

    use super::*;
    use crate::application::build_trigger::{BuildTriggerType, TriggerSource};

    #[test]
    fn test_resolve_cl_link_prefers_existing_cl_link() {
        let context = TriggerContext {
            trigger_type: BuildTriggerType::WebEdit,
            trigger_source: TriggerSource::User,
            triggered_by: Some("jackie".to_string()),
            repo_path: "/project/buck2_test".to_string(),
            cl_path: Some("/project/buck2_test".to_string()),
            from_hash: "1".repeat(40),
            commit_hash: "2".repeat(40),
            cl_link: Some("HVKM7CXI".to_string()),
            cl_id: Some(42),
            params: None,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        };

        assert_eq!(resolve_cl_link(&context, 1_700_000_000_000), "HVKM7CXI");
    }

    #[test]
    fn test_resolve_cl_link_falls_back_to_commit_prefix() {
        let context = TriggerContext {
            trigger_type: BuildTriggerType::WebEdit,
            trigger_source: TriggerSource::User,
            triggered_by: Some("jackie".to_string()),
            repo_path: "/project/buck2_test".to_string(),
            cl_path: Some("/project/buck2_test".to_string()),
            from_hash: "1".repeat(40),
            commit_hash: "abcdef1234567890abcdef1234567890abcdef12".to_string(),
            cl_link: None,
            cl_id: Some(42),
            params: None,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        };

        assert_eq!(
            resolve_cl_link(&context, 1_700_000_000_000),
            "webedit-1700000000000-abcdef12"
        );
    }

    #[test]
    fn test_serialize_builds_to_worker_contract_shape() {
        let builds = vec![
            Status::Modified(ProjectRelativePath::new("src/main.rs")),
            Status::Added(ProjectRelativePath::new("src/generated.rs")),
        ];

        let value = serialize_builds(&builds).expect("serialize builds");
        let roundtrip: Vec<Status<ProjectRelativePath>> =
            serde_json::from_value(value).expect("roundtrip builds json");

        assert_eq!(roundtrip, builds);
    }
}
