use std::time::Duration;

use http::header::AUTHORIZATION;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::client::IntoClientRequest};

use crate::{SchedulerStatusResponse, StartRunnerPayload, StartRunnerSchedulerResponse};

pub type TerminalWebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

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

    /// Open an SSE stream of live Orion runner logs (`GET /logs/orion/stream`).
    ///
    /// Prefer `vm_id`; when absent, `domain` is used. At least one must be set.
    /// The returned response body is a long-lived `text/event-stream` — do not
    /// apply a short request timeout.
    pub async fn stream_orion_logs(
        &self,
        vm_id: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        if vm_id.is_none() && domain.is_none() {
            return Err(anyhow::anyhow!(
                "stream_orion_logs requires vm_id or domain"
            ));
        }

        let mut url = format!("{}/logs/orion/stream?", self.base_url);
        let mut params: Vec<String> = Vec::new();
        if let Some(id) = vm_id {
            params.push(format!("vm_id={}", urlencoding_query(id)));
        }
        if let Some(d) = domain {
            params.push(format!("domain={}", urlencoding_query(d)));
        }
        url.push_str(&params.join("&"));

        let req = self.client.get(&url);
        let res = self.auth_headers(req).send().await?;
        let status = res.status();
        if status.is_success() {
            Ok(res)
        } else {
            let body = res.text().await.unwrap_or_default();
            Err(anyhow::anyhow!(
                "Scheduler stream_orion_logs failed ({}): {}",
                status,
                body
            ))
        }
    }

    /// Open a WebSocket to `GET /vms/{id}/terminal` (vm_id or domain).
    pub async fn connect_terminal(&self, id: &str) -> anyhow::Result<TerminalWebSocket> {
        let ws_base = http_to_ws_base(&self.base_url)?;
        let url = format!("{}/vms/{}/terminal", ws_base, urlencoding_path(id));
        tracing::info!("Connecting terminal WS: {}", url);

        let mut request = url.into_client_request()?;
        if !self.token.is_empty() {
            let value = format!("Bearer {}", self.token);
            request.headers_mut().insert(
                AUTHORIZATION,
                value
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid Authorization header: {e}"))?,
            );
        }

        let (stream, response) = tokio_tungstenite::connect_async(request).await?;
        let status = response.status();
        if !status.is_success() && status.as_u16() != 101 {
            return Err(anyhow::anyhow!(
                "Scheduler connect_terminal handshake failed: {}",
                status
            ));
        }
        Ok(stream)
    }
}

fn http_to_ws_base(base_url: &str) -> anyhow::Result<String> {
    let base = base_url.trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else if let Some(rest) = base.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else if base.starts_with("ws://") || base.starts_with("wss://") {
        Ok(base.to_string())
    } else {
        Err(anyhow::anyhow!(
            "unsupported scheduler URL scheme for terminal WS: {base_url}"
        ))
    }
}

/// Encode a single path segment (vm id / domain).
fn urlencoding_path(value: &str) -> String {
    urlencoding_query(value)
}

fn urlencoding_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_slash_from_base_url() {
        let client = OrionSchedulerHttpClient::new("http://127.0.0.1:8080/", "");
        assert_eq!(client.base_url, "http://127.0.0.1:8080");
    }

    #[test]
    fn urlencoding_query_encodes_reserved_bytes() {
        assert_eq!(urlencoding_query("a b"), "a%20b");
        assert_eq!(urlencoding_query("orion.gitmega.com"), "orion.gitmega.com");
    }

    #[test]
    fn http_to_ws_base_converts_schemes() {
        assert_eq!(
            http_to_ws_base("http://127.0.0.1:8080/").unwrap(),
            "ws://127.0.0.1:8080"
        );
        assert_eq!(
            http_to_ws_base("https://sched.example.com").unwrap(),
            "wss://sched.example.com"
        );
    }
}
