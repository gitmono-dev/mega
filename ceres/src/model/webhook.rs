use callisto::sea_orm_active_enums::WebhookEventTypeEnum;
use jupiter::{sea_orm::ActiveEnum, storage::webhook_storage::WebhookWithEventTypes};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateWebhookRequest {
    pub target_url: String,
    pub secret: String,
    /// Event types: "cl_created", "cl_updated", "cl_merged", "cl_closed", "cl_reopened", "cl_comment_created", "all"
    pub event_types: Vec<String>,
    pub path_filter: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WebhookResponse {
    pub id: i64,
    pub target_url: String,
    pub event_types: Vec<String>,
    pub path_filter: Option<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ListWebhooksQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

impl From<WebhookWithEventTypes> for WebhookResponse {
    fn from(value: WebhookWithEventTypes) -> Self {
        let m = value.webhook;
        Self {
            id: m.id,
            target_url: m.target_url,
            event_types: value
                .event_types
                .into_iter()
                .map(|e| e.to_value().value.into_owned())
                .collect(),
            path_filter: m.path_filter,
            active: m.active,
            created_at: m.created_at.to_string(),
            updated_at: m.updated_at.to_string(),
        }
    }
}

pub fn parse_webhook_event_types(raw: Vec<String>) -> Result<Vec<WebhookEventTypeEnum>, String> {
    raw.into_iter()
        .map(|s| {
            WebhookEventTypeEnum::try_from(s.as_str())
                .map_err(|_| format!("invalid event type: {s}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::WebhookEventTypeEnum;
    use chrono::NaiveDateTime;

    use super::*;

    #[test]
    fn parse_webhook_event_types_accepts_known_values() {
        let parsed = parse_webhook_event_types(vec!["cl_created".to_string(), "all".to_string()])
            .expect("valid event types");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], WebhookEventTypeEnum::ClCreated);
        assert_eq!(parsed[1], WebhookEventTypeEnum::All);
    }

    #[test]
    fn parse_webhook_event_types_rejects_unknown_values() {
        let err = parse_webhook_event_types(vec!["not_a_real_event".to_string()])
            .expect_err("invalid event type");
        assert!(err.contains("invalid event type"));
    }

    #[test]
    fn webhook_response_from_maps_fields() {
        use jupiter::storage::webhook_storage::WebhookWithEventTypes;

        let now =
            NaiveDateTime::parse_from_str("2025-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let webhook = callisto::mega_webhook::Model {
            id: 42,
            target_url: "https://example.com/hook".to_string(),
            secret: "enc".to_string(),
            event_types: "[]".to_string(),
            path_filter: Some("/project".to_string()),
            active: true,
            created_at: now,
            updated_at: now,
        };
        let value = WebhookWithEventTypes {
            webhook,
            event_types: vec![WebhookEventTypeEnum::ClCreated],
        };

        let response = WebhookResponse::from(value);
        assert_eq!(response.id, 42);
        assert_eq!(response.target_url, "https://example.com/hook");
        assert_eq!(response.event_types, vec!["cl_created".to_string()]);
        assert_eq!(response.path_filter.as_deref(), Some("/project"));
        assert!(response.active);
    }
}
