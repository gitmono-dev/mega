use std::{collections::HashMap, fmt};

pub use api_model::buck2::{status::Status, types::ProjectRelativePath};
use callisto::mega_cl;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuildTriggerType {
    GitPush,
    Manual,
    Retry,
    Webhook,
    Schedule,
    WebEdit,
    BuckFileUpload,
}

impl fmt::Display for BuildTriggerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BuildTriggerType::GitPush => "git_push",
            BuildTriggerType::Manual => "manual",
            BuildTriggerType::Retry => "retry",
            BuildTriggerType::Webhook => "webhook",
            BuildTriggerType::Schedule => "schedule",
            BuildTriggerType::WebEdit => "webedit",
            BuildTriggerType::BuckFileUpload => "buck_file_upload",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    User,
    System,
    Service,
}

impl fmt::Display for TriggerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TriggerSource::User => "user",
            TriggerSource::System => "system",
            TriggerSource::Service => "service",
        };
        write!(f, "{}", s)
    }
}

/// Optional build parameters
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BuildParams {
    /// Specific Buck build target (e.g., "//path/to:target")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_target: Option<String>,

    /// Additional build parameters
    #[serde(flatten)]
    #[schema(additional_properties)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitPushPayload {
    pub repo: String,
    pub from_hash: String,
    pub commit_hash: String,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    pub builds: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
}

/// Pure Git Event DTO for decoupling pack layer from build trigger system
#[derive(Debug, Clone)]
pub struct GitPushEvent {
    pub repo_path: String,
    pub from_hash: String,
    pub commit_hash: String,
    pub cl_link: String,
    pub cl_id: Option<i64>,
    pub triggered_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualPayload {
    pub repo: String,
    pub commit_hash: String,
    pub triggered_by: String,
    pub builds: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BuildParams>,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPayload {
    pub repo: String,
    pub from_hash: String,
    pub commit_hash: String,
    pub triggered_by: String,
    pub builds: serde_json::Value,
    pub original_trigger_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_cl_link: Option<String>,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub repo: String,
    pub commit_hash: String,
    pub builds: serde_json::Value,
    pub webhook_source: String,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulePayload {
    pub repo: String,
    pub commit_hash: String,
    pub builds: serde_json::Value,
    pub schedule_name: String,
    pub cron_expression: String,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebEditPayload {
    pub repo: String,
    pub from_hash: String,
    pub commit_hash: String,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    pub builds: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
}

/// Payload for Buck file upload trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckFileUploadPayload {
    pub repo: String,
    pub from_hash: String,
    pub commit_hash: String,
    pub cl_link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    pub builds: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
    /// Target branch name; always "main" for current single-branch upload flow
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// Ref type for display; always "branch" for current flow
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_type: Option<String>,
}

/// Trigger payload - stores context specific to each trigger type
/// This enum is serialized to JSON and stored in database's trigger_payload column
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuildTriggerPayload {
    GitPush(GitPushPayload),
    Manual(ManualPayload),
    Retry(RetryPayload),
    Webhook(WebhookPayload),
    Schedule(SchedulePayload),
    WebEdit(WebEditPayload),
    BuckFileUpload(BuckFileUploadPayload),
}

impl BuildTriggerPayload {
    pub fn repo_path(&self) -> &str {
        match self {
            BuildTriggerPayload::GitPush(p) => &p.repo,
            BuildTriggerPayload::Manual(p) => &p.repo,
            BuildTriggerPayload::Retry(p) => &p.repo,
            BuildTriggerPayload::Webhook(p) => &p.repo,
            BuildTriggerPayload::Schedule(p) => &p.repo,
            BuildTriggerPayload::WebEdit(p) => &p.repo,
            BuildTriggerPayload::BuckFileUpload(p) => &p.repo,
        }
    }

    pub fn commit_hash(&self) -> &str {
        match self {
            BuildTriggerPayload::GitPush(p) => &p.commit_hash,
            BuildTriggerPayload::Manual(p) => &p.commit_hash,
            BuildTriggerPayload::Retry(p) => &p.commit_hash,
            BuildTriggerPayload::Webhook(p) => &p.commit_hash,
            BuildTriggerPayload::Schedule(p) => &p.commit_hash,
            BuildTriggerPayload::WebEdit(p) => &p.commit_hash,
            BuildTriggerPayload::BuckFileUpload(p) => &p.commit_hash,
        }
    }

    pub fn cl_link(&self) -> &str {
        match self {
            BuildTriggerPayload::GitPush(p) => &p.cl_link,
            BuildTriggerPayload::Manual(p) => &p.cl_link,
            BuildTriggerPayload::Retry(p) => &p.cl_link,
            BuildTriggerPayload::Webhook(p) => &p.cl_link,
            BuildTriggerPayload::Schedule(p) => &p.cl_link,
            BuildTriggerPayload::WebEdit(p) => &p.cl_link,
            BuildTriggerPayload::BuckFileUpload(p) => &p.cl_link,
        }
    }

    pub fn cl_id(&self) -> Option<i64> {
        match self {
            BuildTriggerPayload::GitPush(p) => p.cl_id,
            BuildTriggerPayload::Manual(p) => p.cl_id,
            BuildTriggerPayload::Retry(p) => p.cl_id,
            BuildTriggerPayload::Webhook(p) => p.cl_id,
            BuildTriggerPayload::Schedule(p) => p.cl_id,
            BuildTriggerPayload::WebEdit(p) => p.cl_id,
            BuildTriggerPayload::BuckFileUpload(p) => p.cl_id,
        }
    }

    pub fn triggered_by(&self) -> Option<&str> {
        match self {
            BuildTriggerPayload::GitPush(p) => p.triggered_by.as_deref(),
            BuildTriggerPayload::Manual(p) => Some(&p.triggered_by),
            BuildTriggerPayload::Retry(p) => Some(&p.triggered_by),
            BuildTriggerPayload::Webhook(_) => None,
            BuildTriggerPayload::Schedule(_) => None,
            BuildTriggerPayload::WebEdit(p) => p.triggered_by.as_deref(),
            BuildTriggerPayload::BuckFileUpload(p) => p.triggered_by.as_deref(),
        }
    }

    pub fn from_hash(&self) -> &str {
        match self {
            BuildTriggerPayload::GitPush(p) => &p.from_hash,
            BuildTriggerPayload::Manual(p) => &p.commit_hash,
            BuildTriggerPayload::Retry(p) => &p.from_hash,
            BuildTriggerPayload::Webhook(_) => "",
            BuildTriggerPayload::Schedule(_) => "",
            BuildTriggerPayload::WebEdit(p) => &p.from_hash,
            BuildTriggerPayload::BuckFileUpload(p) => &p.from_hash,
        }
    }
}

/// Internal build trigger object (in-memory, used by handlers)
#[derive(Debug, Serialize)]
pub struct BuildTrigger {
    pub trigger_type: BuildTriggerType,
    pub trigger_source: TriggerSource,
    pub trigger_time: DateTime<Utc>,
    pub payload: BuildTriggerPayload,
}

/// Trigger record for history queries (maps to database table)
#[derive(Debug, Serialize, ToSchema)]
pub struct TriggerRecord {
    pub id: i64,
    pub trigger_type: String,
    pub trigger_source: String,
    pub trigger_time: DateTime<Utc>,
    pub trigger_payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub task_id: Option<uuid::Uuid>,
}

impl TriggerRecord {
    pub fn from_db_model(model: callisto::build_triggers::Model) -> Self {
        Self {
            id: model.id,
            trigger_type: model.trigger_type,
            trigger_source: model.trigger_source,
            trigger_time: DateTime::from_naive_utc_and_offset(model.trigger_time, Utc),
            trigger_payload: model.trigger_payload.clone(),
            task_id: model.task_id,
        }
    }

    /// Parse the JSON payload into strongly-typed BuildTriggerPayload
    pub fn parse_payload(&self) -> Result<BuildTriggerPayload, serde_json::Error> {
        serde_json::from_value(self.trigger_payload.clone())
    }
}

/// Unified context for triggering builds from any source
#[derive(Debug, Clone)]
pub struct TriggerContext {
    pub trigger_type: BuildTriggerType,
    pub trigger_source: TriggerSource,
    pub triggered_by: Option<String>,
    pub repo_path: String,
    /// Original CL directory path (may be a subdirectory of [`Self::repo_path`]).
    /// Used to rebase CL-relative change paths onto the Buck repo root.
    pub cl_path: Option<String>,
    pub from_hash: String,
    pub commit_hash: String,
    pub cl_link: Option<String>,
    pub cl_id: Option<i64>,
    pub params: Option<BuildParams>,
    pub original_trigger_id: Option<i64>,
    pub ref_name: Option<String>,
    pub ref_type: Option<String>,
}

/// CL fields needed when constructing a retry [`TriggerContext`].
#[derive(Debug, Clone)]
pub struct RetryClContext {
    pub link: Option<String>,
    pub id: Option<i64>,
    pub path: Option<String>,
}

impl TriggerContext {
    pub fn from_git_push(
        repo_path: String,
        from_hash: String,
        commit_hash: String,
        cl_link: String,
        cl_id: Option<i64>,
        cl_path: Option<String>,
        triggered_by: Option<String>,
    ) -> Self {
        Self {
            trigger_type: BuildTriggerType::GitPush,
            trigger_source: if triggered_by.is_some() {
                TriggerSource::User
            } else {
                TriggerSource::System
            },
            triggered_by,
            repo_path,
            cl_path,
            from_hash,
            commit_hash,
            cl_link: Some(cl_link),
            cl_id,
            params: None,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        }
    }

    pub fn from_manual(
        repo_path: String,
        commit_hash: String,
        triggered_by: String,
        params: Option<BuildParams>,
    ) -> Self {
        Self {
            trigger_type: BuildTriggerType::Manual,
            trigger_source: TriggerSource::User,
            triggered_by: Some(triggered_by),
            repo_path,
            cl_path: None,
            from_hash: commit_hash.clone(),
            commit_hash,
            cl_link: None,
            cl_id: None,
            params,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        }
    }

    pub fn from_retry(
        repo_path: String,
        from_hash: String,
        commit_hash: String,
        cl: RetryClContext,
        triggered_by: Option<String>,
        original_trigger_id: i64,
    ) -> Self {
        Self {
            trigger_type: BuildTriggerType::Retry,
            trigger_source: TriggerSource::User,
            triggered_by,
            repo_path,
            cl_path: cl.path,
            from_hash,
            commit_hash,
            cl_link: cl.link,
            cl_id: cl.id,
            params: None,
            original_trigger_id: Some(original_trigger_id),
            ref_name: None,
            ref_type: None,
        }
    }

    /// Build context for Buck file upload trigger.
    /// Caller should set context.ref_name and context.ref_type after construction (same as Manual).
    pub fn from_buck_upload(
        repo_path: String,
        from_hash: String,
        commit_hash: String,
        cl_link: String,
        cl_id: Option<i64>,
        cl_path: Option<String>,
        triggered_by: Option<String>,
    ) -> Self {
        Self {
            trigger_type: BuildTriggerType::BuckFileUpload,
            trigger_source: if triggered_by.is_some() {
                TriggerSource::User
            } else {
                TriggerSource::System
            },
            triggered_by,
            repo_path,
            cl_path,
            from_hash,
            commit_hash,
            cl_link: Some(cl_link),
            cl_id,
            params: None,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        }
    }
}

impl From<mega_cl::Model> for TriggerContext {
    fn from(cl: mega_cl::Model) -> Self {
        TriggerContext {
            trigger_type: BuildTriggerType::WebEdit,
            trigger_source: TriggerSource::User,
            triggered_by: Some(cl.username),
            repo_path: cl.path.clone(),
            cl_path: Some(cl.path),
            from_hash: cl.from_hash,
            commit_hash: cl.to_hash,
            cl_link: Some(cl.link),
            cl_id: Some(cl.id),
            params: None,
            original_trigger_id: None,
            ref_name: None,
            ref_type: None,
        }
    }
}

/// Create trigger request (new RESTful API)
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTriggerRequest {
    pub repo_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<BuildParams>,
}

/// Trigger detail response (new RESTful API)
#[derive(Debug, Serialize, ToSchema)]
pub struct TriggerResponse {
    pub id: i64,
    pub trigger_type: String,
    pub trigger_source: String,
    pub triggered_by: Option<String>,
    pub triggered_at: DateTime<Utc>,
    pub repo_path: String,
    pub commit_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub task_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_trigger_id: Option<i64>,
}

impl TriggerResponse {
    /// Create TriggerResponse from database model
    pub fn from_trigger_record(record: &TriggerRecord) -> Result<Self, serde_json::Error> {
        let payload = record.parse_payload()?;

        // Extract common fields using helper methods
        let repo_path = payload.repo_path().to_string();
        let commit_hash = payload.commit_hash().to_string();
        let cl_link = Some(payload.cl_link().to_string());
        let cl_id = payload.cl_id();
        let triggered_by = payload.triggered_by().map(|s| s.to_string());

        // Extract type-specific fields
        let (params, original_trigger_id, ref_name, ref_type) = match &payload {
            BuildTriggerPayload::Manual(p) => (
                p.params.as_ref().and_then(|p| serde_json::to_value(p).ok()),
                None,
                p.ref_name.clone(),
                p.ref_type.clone(),
            ),
            BuildTriggerPayload::Retry(p) => (
                None,
                Some(p.original_trigger_id),
                p.ref_name.clone(),
                p.ref_type.clone(),
            ),
            BuildTriggerPayload::Webhook(p) => (p.raw_payload.clone(), None, None, None),
            BuildTriggerPayload::BuckFileUpload(p) => {
                (None, None, p.ref_name.clone(), p.ref_type.clone())
            }
            _ => (None, None, None, None),
        };

        Ok(Self {
            id: record.id,
            trigger_type: record.trigger_type.clone(),
            trigger_source: record.trigger_source.clone(),
            triggered_by,
            triggered_at: record.trigger_time,
            repo_path,
            commit_hash,
            ref_name,
            ref_type,
            cl_link,
            cl_id,
            params,
            task_id: record.task_id,
            original_trigger_id,
        })
    }
}

/// Query parameters for listing triggers (Google-style API)
#[derive(Debug, Deserialize, ToSchema, Default)]
pub struct ListTriggersParams {
    /// Filter by repository path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    /// Filter by trigger type (git_push, manual, retry, webhook, schedule, buck_file_upload)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_type: Option<String>,
    /// Filter by trigger source (user, system, service)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_source: Option<String>,
    /// Filter by who triggered (username)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
    /// Filter by time range start (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    /// Filter by time range end (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

impl From<ListTriggersParams> for jupiter::storage::build_trigger_storage::ListTriggersFilter {
    fn from(params: ListTriggersParams) -> Self {
        Self {
            repo_path: params.repo_path,
            trigger_type: params.trigger_type,
            trigger_source: params.trigger_source,
            triggered_by: params.triggered_by,
            start_time: params.start_time,
            end_time: params.end_time,
        }
    }
}
