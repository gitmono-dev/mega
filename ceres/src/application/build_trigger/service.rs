//! # Build Trigger Service
//!
//! Central entry point for all CI/CD build triggers within the Mega system.
//! All modules requiring a build (e.g., Git Push, Web IDE, API) must route their
//! requests through this service.
//!
//! ## Common Usage Scenarios
//!
//! ### 1. Code Push (Git Flow)
//! When a user pushes code via Git, use:
//! `service.trigger_for_git_push(event)`
//!
//! ### 2. Web IDE / Change List (CL Flow)
//! For creating or editing files via the web interface:
//! - If you have the CL link: `service.trigger_for_cl(cl_link)`
//! - If you have the CL model: `service.trigger_for_cl_model(cl)`
//!
//! ### 3. Administrative / Manual Ops
//! For UI-driven manual triggers or retries:
//! - Manual: `service.create_manual_trigger(...)`
//! - Retry: `service.retry_trigger(...)`
//!
//! ## Architecture Note
//! This service acts as a Facade, encapsulating internal logic such as change
//! calculation and Orion integration. External callers should only interact
//! with this service and never touch `TriggerRegistry` or `TriggerContext` directly.

use std::sync::Arc;

use api_model::common::Pagination;
use common::errors::MegaError;
use jupiter::storage::Storage;

use super::model::{
    BuildParams, GitPushEvent, ListTriggersParams, RetryClContext, TriggerContext, TriggerRecord,
    TriggerResponse,
};
use crate::application::{
    api_service::cache::GitObjectCache,
    build_trigger::{ChangesPort, RefResolver, SharedBuildDispatch, TriggerRegistry},
    code_edit::utils as edit_utils,
};

/// Service for orchestrating build trigger operations from various sources.
pub struct BuildTriggerService {
    storage: Storage,
    registry: TriggerRegistry,
    build_dispatch: SharedBuildDispatch,
}

impl BuildTriggerService {
    async fn context_from_cl(
        storage: &Storage,
        cl: callisto::mega_cl::Model,
    ) -> Result<TriggerContext, MegaError> {
        let repo_path = edit_utils::resolve_build_repo_root(storage, &cl.path).await?;
        let mut context: TriggerContext = cl.into();
        context.repo_path = repo_path;
        Ok(context)
    }

    pub fn new(
        storage: Storage,
        build_dispatch: SharedBuildDispatch,
        changes_port: Arc<dyn ChangesPort>,
    ) -> Self {
        let registry = TriggerRegistry::new(storage.clone(), build_dispatch.clone(), changes_port);
        Self {
            storage,
            registry,
            build_dispatch,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.build_dispatch.enable_build()
    }

    fn check_build_enabled(&self) -> Result<(), MegaError> {
        if !self.is_enabled() {
            return Err(MegaError::unavailable("Build system is not enabled"));
        }
        Ok(())
    }

    /// Triggers a build based on a Git push event.
    pub async fn trigger_for_git_push(
        &self,
        event: GitPushEvent,
    ) -> Result<Option<i64>, MegaError> {
        if !self.is_enabled() {
            return Ok(None);
        }

        let cl_path = Some(event.repo_path.clone());
        let context = TriggerContext::from_git_push(
            event.repo_path,
            event.from_hash,
            event.commit_hash,
            event.cl_link,
            event.cl_id,
            cl_path,
            event.triggered_by,
        );

        let id = self.registry.trigger_build(context).await?;
        Ok(Some(id))
    }

    pub async fn build_by_context(
        storage: Storage,
        git_object_cache: Arc<GitObjectCache>,
        build_dispatch: SharedBuildDispatch,
        context: TriggerContext,
    ) -> Result<Option<i64>, MegaError> {
        if !build_dispatch.enable_build() {
            return Ok(None);
        }
        let (_, cl, _) = crate::application::api_service::mono::stack::build_mono_stack(
            storage.clone(),
            git_object_cache,
            Some(build_dispatch.clone()),
        );
        let registry = TriggerRegistry::new(
            storage,
            build_dispatch,
            Arc::new(cl) as Arc<dyn ChangesPort>,
        );

        let id = registry.trigger_build(context).await?;
        Ok(Some(id))
    }

    /// Triggers a build for an existing CL using its unique link.
    pub async fn trigger_for_cl(&self, cl_link: &str) -> Result<Option<i64>, MegaError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        let cl = self
            .storage
            .cl_storage()
            .get_cl(cl_link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL not found: {cl_link}")))?;
        let context = Self::context_from_cl(&self.storage, cl).await?;
        let id = self.registry.trigger_build(context).await?;
        Ok(Some(id))
    }

    /// Triggers a build using an existing CL model to avoid redundant DB lookups.
    pub async fn trigger_for_cl_model(
        &self,
        cl: callisto::mega_cl::Model,
    ) -> Result<Option<i64>, MegaError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        let context = Self::context_from_cl(&self.storage, cl).await?;
        let id = self.registry.trigger_build(context).await?;
        Ok(Some(id))
    }

    /// Facilitates a manual build trigger, including reference resolution.
    pub async fn create_manual_trigger(
        &self,
        repo_path: String,
        ref_name: Option<String>,
        params: Option<BuildParams>,
        triggered_by: String,
    ) -> Result<TriggerResponse, MegaError> {
        self.check_build_enabled()?;

        let ref_resolver = RefResolver::new(self.storage.clone());
        let resolved = ref_resolver
            .resolve(ref_name.as_deref())
            .await
            .map_err(|_| {
                let ref_str = ref_name.unwrap_or_else(|| "main".to_string());
                MegaError::NotFound(format!("Reference not found: {ref_str}"))
            })?;

        let mut context = TriggerContext::from_manual(
            repo_path,
            resolved.commit_hash.clone(),
            triggered_by,
            params,
        );
        context.ref_name = Some(resolved.ref_name.clone());
        context.ref_type = Some(resolved.ref_type.as_str().to_string());

        let trigger_id = self.registry.trigger_build(context).await?;
        self.get_trigger_response(trigger_id).await
    }

    /// Creates a new trigger entry based on an existing build history.
    pub async fn retry_trigger(
        &self,
        original_trigger_id: i64,
        triggered_by: String,
    ) -> Result<TriggerResponse, MegaError> {
        self.check_build_enabled()?;

        let original_trigger = self
            .storage
            .build_trigger_storage()
            .get_by_id(original_trigger_id)
            .await?
            .ok_or_else(|| {
                MegaError::NotFound(format!("Trigger not found: {original_trigger_id}"))
            })?;

        let trigger_record = TriggerRecord::from_db_model(original_trigger);
        let payload = trigger_record
            .parse_payload()
            .map_err(|e| MegaError::Other(format!("Failed to parse payload: {}", e)))?;

        // Historical payloads only store Buck repo_path; reload CL path so retry
        // can rebase CL-relative change paths onto the monorepo root.
        let cl_path = match self.storage.cl_storage().get_cl(payload.cl_link()).await? {
            Some(cl) => Some(cl.path),
            None => None,
        };

        let context = TriggerContext::from_retry(
            payload.repo_path().to_string(),
            payload.from_hash().to_string(),
            payload.commit_hash().to_string(),
            RetryClContext {
                link: Some(payload.cl_link().to_string()),
                id: payload.cl_id(),
                path: cl_path,
            },
            Some(triggered_by),
            original_trigger_id,
        );

        let new_trigger_id = self.registry.trigger_build(context).await?;
        self.get_trigger_response(new_trigger_id).await
    }

    /// Retrieves detailed status and information of a specific trigger.
    pub async fn get_trigger(&self, trigger_id: i64) -> Result<TriggerResponse, MegaError> {
        self.check_build_enabled()?;
        self.get_trigger_response(trigger_id).await
    }

    /// Returns a list of triggers with support for filtering and pagination.
    pub async fn list_triggers(
        &self,
        params: ListTriggersParams,
        pagination: Pagination,
    ) -> Result<(Vec<TriggerResponse>, i64), MegaError> {
        self.check_build_enabled()?;

        let (triggers, total) = self
            .storage
            .build_trigger_storage()
            .get_trigger_list(params, pagination)
            .await?;

        let responses = triggers
            .into_iter()
            .map(|t| {
                let record = TriggerRecord::from_db_model(t);
                TriggerResponse::from_trigger_record(&record)
                    .map_err(|e| MegaError::Other(format!("Failed to parse trigger: {}", e)))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((responses, total as i64))
    }

    /// Low-level interface for direct build triggering via context.
    #[allow(dead_code)]
    pub(crate) async fn trigger_with_context(
        &self,
        context: TriggerContext,
    ) -> Result<i64, MegaError> {
        self.check_build_enabled()?;
        self.registry.trigger_build(context).await
    }

    async fn get_trigger_response(&self, trigger_id: i64) -> Result<TriggerResponse, MegaError> {
        let model = self
            .storage
            .build_trigger_storage()
            .get_by_id(trigger_id)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("Trigger not found: {trigger_id}")))?;

        let record = TriggerRecord::from_db_model(model);
        TriggerResponse::from_trigger_record(&record)
            .map_err(|e| MegaError::Other(format!("Failed to parse trigger: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::application::build_trigger::BuildTriggerType;

    #[tokio::test]
    async fn test_context_from_cl_resolves_repo_root_from_registered_repo_path() {
        let temp_dir = tempdir().expect("create temp dir");
        let storage = jupiter::tests::test_storage(temp_dir.path()).await;
        storage
            .git_db_storage()
            .create_repo_and_save_ref(
                "/project/buck2_test",
                "buck2_test",
                "refs/heads/main",
                "main",
            )
            .await
            .expect("register repo path");

        let cl = storage
            .cl_storage()
            .new_cl_model(
                "/project/buck2_test/src",
                "HVKM7CXI",
                "web edit",
                "main",
                &"1".repeat(40),
                &"2".repeat(40),
                "jackie",
            )
            .await
            .expect("insert cl");

        let context = BuildTriggerService::context_from_cl(&storage, cl)
            .await
            .expect("resolve cl context");

        assert_eq!(context.repo_path, "/project/buck2_test");
        assert_eq!(context.cl_path.as_deref(), Some("/project/buck2_test/src"));
        assert_eq!(context.cl_link.as_deref(), Some("HVKM7CXI"));
        assert_eq!(context.trigger_type, BuildTriggerType::WebEdit);
    }

    #[tokio::test]
    async fn test_context_from_cl_returns_error_when_repo_root_unresolvable() {
        let temp_dir = tempdir().expect("create temp dir");
        let storage = jupiter::tests::test_storage(temp_dir.path()).await;
        let cl = storage
            .cl_storage()
            .new_cl_model(
                "/unknown/repo/src",
                "UNKNOWNCL",
                "web edit",
                "main",
                &"1".repeat(40),
                &"2".repeat(40),
                "jackie",
            )
            .await
            .expect("insert cl");

        let err = BuildTriggerService::context_from_cl(&storage, cl)
            .await
            .expect_err("repo root should be missing");

        assert!(err.to_string().contains("No repository root found"));
    }
}
