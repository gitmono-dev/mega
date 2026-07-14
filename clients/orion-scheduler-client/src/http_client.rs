use std::time::Duration;

use crate::{SchedulerStatusResponse, StartRunnerPayload, StartRunnerSchedulerResponse};

#[derive(Clone)]
pub struct OrionSchedulerHttpClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl OrionSchedulerHttpClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let use_direct_connection = base_url.starts_with("http://127.0.0.1")
            || base_url.starts_with("https://127.0.0.1")
            || base_url.starts_with("http://localhost")
            || base_url.starts_with("https://localhost")
            || base_url.starts_with("http://[::1]")
            || base_url.starts_with("https://[::1]");
        let client = if use_direct_connection {
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        } else {
            reqwest::Client::new()
        };

        Self {
            base_url,
            token: token.into(),
            client,
        }
    }

    fn auth_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.token.is_empty() {
            builder
        } else {
            builder.header("Authorization", format!("Bearer {}", self.token))
        }
    }

    pub async fn start_runner(
        &self,
        payload: StartRunnerPayload,
    ) -> anyhow::Result<StartRunnerSchedulerResponse> {
        let url = format!("{}/webhook", self.base_url);
        tracing::info!(
            "Starting runner via scheduler: server_ws={}",
            payload.server_ws
        );
        let req = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(10))
            .json(&payload);
        let res = self.auth_headers(req).send().await?;
        let status = res.status();
        let body: StartRunnerSchedulerResponse = res.json().await?;
        // 200 OK (idempotent), 202 Accepted (provisioning), 409 Conflict
        if status.is_success() || status.as_u16() == 202 || status.as_u16() == 409 {
            Ok(body)
        } else {
            Err(anyhow::anyhow!(
                "Scheduler start_runner failed ({}): {}",
                status,
                body.error.unwrap_or_else(|| body.status.clone())
            ))
        }
    }

    pub async fn get_vm_status(&self, vm_id: &str) -> anyhow::Result<SchedulerStatusResponse> {
        let url = format!("{}/vms/{}", self.base_url, vm_id);
        let req = self.client.get(&url).timeout(Duration::from_secs(30));
        let res = self.auth_headers(req).send().await?;
        let status = res.status();
        if status.as_u16() == 404 {
            return Ok(SchedulerStatusResponse {
                status: "no_vm".to_string(),
                phase: Some("no_vm".to_string()),
                vm_id: Some(vm_id.to_string()),
                domain: None,
                vm_ip: None,
                uptime_secs: None,
                log_file: None,
                error: Some("VM not found".to_string()),
            });
        }
        if status.is_success() {
            Ok(res.json().await?)
        } else {
            Err(anyhow::anyhow!(
                "Scheduler get_vm_status failed: {}",
                status
            ))
        }
    }

    pub async fn get_status(&self) -> anyhow::Result<SchedulerStatusResponse> {
        let url = format!("{}/status", self.base_url);
        let req = self.client.get(&url).timeout(Duration::from_secs(30));
        let res = self.auth_headers(req).send().await?;
        if res.status().is_success() {
            // List form — not used by mono GET by id path anymore.
            let v: serde_json::Value = res.json().await?;
            if let Some(vms) = v.get("vms").and_then(|x| x.as_array()) {
                if let Some(first) = vms.first() {
                    return Ok(serde_json::from_value(first.clone())?);
                }
                return Ok(SchedulerStatusResponse {
                    status: "no_vm".to_string(),
                    phase: Some("no_vm".to_string()),
                    vm_id: None,
                    domain: None,
                    vm_ip: None,
                    uptime_secs: None,
                    log_file: None,
                    error: None,
                });
            }
            Ok(serde_json::from_value(v)?)
        } else {
            Err(anyhow::anyhow!(
                "Scheduler get_status failed: {}",
                res.status()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_slash_from_base_url() {
        let client = OrionSchedulerHttpClient::new("http://127.0.0.1:8080/", "");
        assert_eq!(client.base_url, "http://127.0.0.1:8080");
    }
}
