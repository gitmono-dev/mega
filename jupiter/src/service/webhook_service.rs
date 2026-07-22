use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use callisto::{
    mega_cl,
    sea_orm_active_enums::{MergeStatusEnum, WebhookEventTypeEnum},
};
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use idgenerator::IdInstance;
use reqwest::redirect::Policy;
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use sea_orm::ActiveEnum;
use serde::Serialize;
use sha2::Sha256;
use tokio::net::lookup_host;
use url::{Host, Url};

use crate::storage::webhook_storage::WebhookStorage;

const WEBHOOK_SECRET_ENC_PREFIX: &str = "enc:v1:";
const WEBHOOK_SECRET_ENC_KEY_ENV: &str = "MEGA_WEBHOOK_SECRET_ENC_KEY";
const WEBHOOK_SECRET_KEY_LEN: usize = 32;
const WEBHOOK_SECRET_NONCE_LEN: usize = 12;
const WEBHOOK_DELIVERY_MAX_ATTEMPTS: i32 = 3;
const WEBHOOK_DELIVERY_BACKOFF_BASE_SECS: u64 = 2;

pub type WebhookEvent = WebhookEventTypeEnum;

#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub mega_version: String,
    pub event: String,
    pub timestamp: i64,
    pub cl: ClPayload,
    pub repository: RepositoryPayload,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositoryPayload {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorPayload {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClPayload {
    pub id: i64,
    pub link: String,
    pub title: String,
    pub author: AuthorPayload,
    pub status: String,
    pub path: String,
    pub base_branch: String,
    pub head_commit: String,
}

impl From<&mega_cl::Model> for ClPayload {
    fn from(model: &mega_cl::Model) -> Self {
        Self {
            id: model.id,
            link: model.link.clone(),
            title: model.title.clone(),
            author: AuthorPayload {
                name: model.username.clone(),
            },
            status: merge_status_to_str(&model.status).to_string(),
            path: model.path.clone(),
            base_branch: model.base_branch.clone(),
            head_commit: model.to_hash.clone(),
        }
    }
}

#[derive(Clone)]
pub struct WebhookService {
    storage: WebhookStorage,
    client: reqwest::Client,
}

impl WebhookService {
    pub fn new(storage: WebhookStorage) -> Result<Self, common::errors::MegaError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .build()
            .map_err(|e| {
                common::errors::MegaError::Other(format!("failed to build reqwest client: {e}"))
            })?;
        Ok(Self { storage, client })
    }

    pub fn mock(storage: WebhookStorage) -> Self {
        Self {
            storage,
            client: reqwest::Client::new(),
        }
    }

    pub fn dispatch(&self, event_type: WebhookEvent, cl_model: &mega_cl::Model) {
        let svc = self.clone();
        let cl_payload = ClPayload::from(cl_model);
        let path = cl_model.path.clone();

        tokio::spawn(async move {
            if let Err(e) = svc.dispatch_inner(event_type, cl_payload, &path).await {
                tracing::error!("webhook dispatch error: {e}");
            }
        });
    }

    async fn dispatch_inner(
        &self,
        event_type: WebhookEvent,
        cl_payload: ClPayload,
        path: &str,
    ) -> Result<(), common::errors::MegaError> {
        let event_type_str = event_type.to_value().value.into_owned();
        let webhooks = self
            .storage
            .find_matching_webhooks(event_type.clone(), path)
            .await?;

        for webhook in webhooks {
            let repo_name = extract_repo_name(path).to_string();
            let payload = WebhookPayload {
                mega_version: env!("CARGO_PKG_VERSION").to_string(),
                event: event_type_str.clone(),
                timestamp: Utc::now().timestamp(),
                cl: cl_payload.clone(),
                repository: RepositoryPayload {
                    path: path.to_string(),
                    name: repo_name,
                },
            };

            let payload_json = serde_json::to_string(&payload)?;

            let mut last_failure_message = None;
            let mut delivered_successfully = false;
            for attempt in 1..=WEBHOOK_DELIVERY_MAX_ATTEMPTS {
                let should_retry = match self
                    .deliver(
                        &webhook.target_url,
                        &webhook.secret,
                        &event_type_str,
                        &payload_json,
                    )
                    .await
                {
                    Ok((status, body)) => {
                        let success = (200..300).contains(&status);
                        let error_message = if success {
                            None
                        } else {
                            Some(format!("webhook endpoint returned HTTP {status}"))
                        };
                        let delivery = callisto::mega_webhook_delivery::Model {
                            id: IdInstance::next_id(),
                            webhook_id: webhook.id,
                            event_type: event_type.clone(),
                            payload: payload_json.clone(),
                            response_status: Some(status as i32),
                            response_body: Some(body),
                            success,
                            attempt,
                            error_message: error_message.clone(),
                            created_at: Utc::now().naive_utc(),
                        };
                        if let Err(e) = self.storage.save_delivery(delivery).await {
                            tracing::warn!("failed to save webhook delivery record: {e}");
                        }
                        if success {
                            delivered_successfully = true;
                            last_failure_message = None;
                            break;
                        }
                        last_failure_message = error_message;
                        true
                    }
                    Err(e) => {
                        let error_message = e.to_string();
                        let delivery = callisto::mega_webhook_delivery::Model {
                            id: IdInstance::next_id(),
                            webhook_id: webhook.id,
                            event_type: event_type.clone(),
                            payload: payload_json.clone(),
                            response_status: None,
                            response_body: None,
                            success: false,
                            attempt,
                            error_message: Some(error_message.clone()),
                            created_at: Utc::now().naive_utc(),
                        };
                        if let Err(save_err) = self.storage.save_delivery(delivery).await {
                            tracing::warn!("failed to save webhook delivery record: {save_err}");
                        }
                        last_failure_message = Some(error_message);
                        true
                    }
                };

                if should_retry && attempt < WEBHOOK_DELIVERY_MAX_ATTEMPTS {
                    let backoff =
                        Duration::from_secs(WEBHOOK_DELIVERY_BACKOFF_BASE_SECS.pow(attempt as u32));
                    tokio::time::sleep(backoff).await;
                }
            }

            if !delivered_successfully && let Some(error_message) = last_failure_message {
                tracing::warn!(
                    "webhook delivery failed after {} attempts for webhook_id={}: {}",
                    WEBHOOK_DELIVERY_MAX_ATTEMPTS,
                    webhook.id,
                    error_message
                );
            }
        }

        Ok(())
    }

    async fn deliver(
        &self,
        url: &str,
        secret: &str,
        event_type: &str,
        payload: &str,
    ) -> Result<(u16, String), common::errors::MegaError> {
        validate_webhook_target_url_for_delivery(url).await?;
        let signing_secret = decrypt_webhook_secret(secret)?;
        let signature = compute_hmac_signature(&signing_secret, payload);

        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Mega-Event", event_type)
            .header("X-Mega-Signature", format!("sha256={signature}"))
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| common::errors::MegaError::Other(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok((status, body))
    }
}

fn merge_status_to_str(status: &MergeStatusEnum) -> &'static str {
    match status {
        MergeStatusEnum::Open => "open",
        MergeStatusEnum::Merged => "merged",
        MergeStatusEnum::Closed => "closed",
        MergeStatusEnum::Draft => "draft",
    }
}

fn extract_repo_name(path: &str) -> &str {
    path.trim_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("unknown")
}

fn compute_hmac_signature(secret: &str, payload: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn validate_webhook_target_url(target_url: &str) -> Result<(), common::errors::MegaError> {
    let url = parse_webhook_url(target_url)?;
    validate_webhook_url_static(&url)
}

pub fn encrypt_webhook_secret(secret: &str) -> Result<String, common::errors::MegaError> {
    if secret.is_empty() {
        return Err(common::errors::MegaError::Other(
            "webhook secret cannot be empty".to_string(),
        ));
    }

    let key_bytes = load_webhook_secret_key_from_env()?;
    encrypt_webhook_secret_with_key(secret, &key_bytes)
}

pub fn decrypt_webhook_secret(secret: &str) -> Result<String, common::errors::MegaError> {
    if !secret.starts_with(WEBHOOK_SECRET_ENC_PREFIX) {
        // Backward compatibility for existing plaintext secrets.
        return Ok(secret.to_string());
    }

    let key_bytes = load_webhook_secret_key_from_env()?;
    decrypt_webhook_secret_with_key(secret, &key_bytes)
}

async fn validate_webhook_target_url_for_delivery(
    target_url: &str,
) -> Result<(), common::errors::MegaError> {
    let url = parse_webhook_url(target_url)?;
    validate_webhook_url_static(&url)?;
    validate_resolved_ips(&url).await
}

fn parse_webhook_url(target_url: &str) -> Result<Url, common::errors::MegaError> {
    Url::parse(target_url)
        .map_err(|e| common::errors::MegaError::Other(format!("invalid webhook target_url: {e}")))
}

fn load_webhook_secret_key_from_env()
-> Result<[u8; WEBHOOK_SECRET_KEY_LEN], common::errors::MegaError> {
    let encoded_key = std::env::var(WEBHOOK_SECRET_ENC_KEY_ENV).map_err(|_| {
        common::errors::MegaError::Other(format!(
            "{WEBHOOK_SECRET_ENC_KEY_ENV} is not set for webhook secret encryption"
        ))
    })?;

    let decoded_key = BASE64_STANDARD.decode(encoded_key).map_err(|_| {
        common::errors::MegaError::Other(format!(
            "{WEBHOOK_SECRET_ENC_KEY_ENV} must be a valid base64-encoded 32-byte key"
        ))
    })?;

    let key: [u8; WEBHOOK_SECRET_KEY_LEN] = decoded_key.try_into().map_err(|_| {
        common::errors::MegaError::Other(format!(
            "{WEBHOOK_SECRET_ENC_KEY_ENV} must decode to exactly {} bytes",
            WEBHOOK_SECRET_KEY_LEN
        ))
    })?;
    Ok(key)
}

fn encrypt_webhook_secret_with_key(
    secret: &str,
    key_bytes: &[u8; WEBHOOK_SECRET_KEY_LEN],
) -> Result<String, common::errors::MegaError> {
    let unbound_key = UnboundKey::new(&aead::AES_256_GCM, key_bytes).map_err(|_| {
        common::errors::MegaError::Other("invalid webhook encryption key".to_string())
    })?;
    let key = LessSafeKey::new(unbound_key);

    let mut nonce_bytes = [0u8; WEBHOOK_SECRET_NONCE_LEN];
    SystemRandom::new().fill(&mut nonce_bytes).map_err(|_| {
        common::errors::MegaError::Other("failed to generate webhook secret nonce".to_string())
    })?;

    let mut in_out = secret.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::empty(),
        &mut in_out,
    )
    .map_err(|_| {
        common::errors::MegaError::Other("failed to encrypt webhook secret".to_string())
    })?;

    let mut payload = Vec::with_capacity(WEBHOOK_SECRET_NONCE_LEN + in_out.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&in_out);

    Ok(format!(
        "{WEBHOOK_SECRET_ENC_PREFIX}{}",
        BASE64_STANDARD.encode(payload)
    ))
}

fn decrypt_webhook_secret_with_key(
    encrypted_secret: &str,
    key_bytes: &[u8; WEBHOOK_SECRET_KEY_LEN],
) -> Result<String, common::errors::MegaError> {
    let encoded_payload = encrypted_secret
        .strip_prefix(WEBHOOK_SECRET_ENC_PREFIX)
        .ok_or_else(|| {
            common::errors::MegaError::Other("invalid encrypted webhook secret prefix".to_string())
        })?;

    let payload = BASE64_STANDARD.decode(encoded_payload).map_err(|_| {
        common::errors::MegaError::Other("invalid encrypted webhook secret format".to_string())
    })?;

    if payload.len() <= WEBHOOK_SECRET_NONCE_LEN {
        return Err(common::errors::MegaError::Other(
            "invalid encrypted webhook secret payload".to_string(),
        ));
    }

    let mut nonce_bytes = [0u8; WEBHOOK_SECRET_NONCE_LEN];
    nonce_bytes.copy_from_slice(&payload[..WEBHOOK_SECRET_NONCE_LEN]);
    let mut cipher_and_tag = payload[WEBHOOK_SECRET_NONCE_LEN..].to_vec();

    let unbound_key = UnboundKey::new(&aead::AES_256_GCM, key_bytes).map_err(|_| {
        common::errors::MegaError::Other("invalid webhook encryption key".to_string())
    })?;
    let key = LessSafeKey::new(unbound_key);

    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut cipher_and_tag,
        )
        .map_err(|_| {
            common::errors::MegaError::Other("failed to decrypt webhook secret".to_string())
        })?;

    String::from_utf8(plaintext.to_vec()).map_err(|_| {
        common::errors::MegaError::Other("decrypted webhook secret is not valid UTF-8".to_string())
    })
}

fn validate_webhook_url_static(url: &Url) -> Result<(), common::errors::MegaError> {
    if url.scheme() != "https" {
        return Err(common::errors::MegaError::Other(
            "webhook target_url must use https".to_string(),
        ));
    }

    let host = url.host().ok_or_else(|| {
        common::errors::MegaError::Other("webhook target_url must include a host".to_string())
    })?;

    match host {
        Host::Domain(domain) => {
            let domain_lc = domain.to_ascii_lowercase();
            if domain_lc == "localhost"
                || domain_lc.ends_with(".localhost")
                || domain_lc.ends_with(".local")
            {
                return Err(common::errors::MegaError::Other(
                    "webhook target_url host is not allowed".to_string(),
                ));
            }
        }
        Host::Ipv4(ip) => {
            if is_forbidden_ip(IpAddr::V4(ip)) {
                return Err(common::errors::MegaError::Other(
                    "webhook target_url IP is not allowed".to_string(),
                ));
            }
        }
        Host::Ipv6(ip) => {
            if is_forbidden_ip(IpAddr::V6(ip)) {
                return Err(common::errors::MegaError::Other(
                    "webhook target_url IP is not allowed".to_string(),
                ));
            }
        }
    }

    Ok(())
}

async fn validate_resolved_ips(url: &Url) -> Result<(), common::errors::MegaError> {
    if matches!(url.host(), Some(Host::Ipv4(_) | Host::Ipv6(_))) {
        return Ok(());
    }

    let host = url.host_str().ok_or_else(|| {
        common::errors::MegaError::Other("webhook target_url must include a host".to_string())
    })?;
    let port = url.port_or_known_default().ok_or_else(|| {
        common::errors::MegaError::Other("webhook target_url must include a valid port".to_string())
    })?;

    let mut resolved_any = false;
    let addrs = lookup_host((host, port)).await.map_err(|e| {
        common::errors::MegaError::Other(format!("failed to resolve webhook host: {e}"))
    })?;

    for addr in addrs {
        resolved_any = true;
        if is_forbidden_ip(addr.ip()) {
            return Err(common::errors::MegaError::Other(
                "webhook target_url resolves to a restricted IP".to_string(),
            ));
        }
    }

    if !resolved_any {
        return Err(common::errors::MegaError::Other(
            "webhook target_url host did not resolve to any address".to_string(),
        ));
    }

    Ok(())
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_ipv4(v4),
        IpAddr::V6(v6) => is_forbidden_ipv6(v6),
    }
}

fn is_forbidden_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (18..=19).contains(&b))
}

fn is_forbidden_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unicast_link_local()
        || ip.is_unique_local()
        || ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::MergeStatusEnum;
    use chrono::DateTime;

    use super::*;

    #[test]
    fn test_validate_webhook_target_url_accepts_public_https_domain() {
        let result = validate_webhook_target_url("https://example.com/webhook");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_non_https() {
        let result = validate_webhook_target_url("http://example.com/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_loopback_ip() {
        let result = validate_webhook_target_url("https://127.0.0.1:8443/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_private_ipv4() {
        let result = validate_webhook_target_url("https://192.168.1.2/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_localhost_domain() {
        let result = validate_webhook_target_url("https://localhost/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_loopback_ipv6() {
        let result = validate_webhook_target_url("https://[::1]/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_webhook_target_url_rejects_link_local_ipv6() {
        let result = validate_webhook_target_url("https://[fe80::1]/webhook");
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_decrypt_webhook_secret_round_trip() {
        let key = [7u8; 32];
        let secret = "test-webhook-secret";

        let encrypted = encrypt_webhook_secret_with_key(secret, &key).expect("encrypt failed");
        assert!(encrypted.starts_with("enc:v1:"));

        let decrypted = decrypt_webhook_secret_with_key(&encrypted, &key).expect("decrypt failed");
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_decrypt_webhook_secret_plaintext_is_backward_compatible() {
        let plaintext = "legacy-plaintext-secret";
        let decrypted = decrypt_webhook_secret(plaintext).expect("decrypt failed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_extract_repo_name_normal_path() {
        assert_eq!(extract_repo_name("/mono/repo"), "repo");
    }

    #[test]
    fn test_extract_repo_name_trailing_slash() {
        assert_eq!(extract_repo_name("/mono/repo/"), "repo");
    }

    #[test]
    fn test_extract_repo_name_root_path_fallback() {
        assert_eq!(extract_repo_name("/"), "unknown");
    }

    #[test]
    fn test_extract_repo_name_empty_path_fallback() {
        assert_eq!(extract_repo_name(""), "unknown");
    }

    #[test]
    fn test_cl_payload_from_model_uses_contract_fields() {
        let model = callisto::mega_cl::Model {
            id: 1,
            link: "CL123".to_string(),
            title: "Test".to_string(),
            merge_date: None,
            status: MergeStatusEnum::Open,
            path: "/repo/path".to_string(),
            base_branch: "main".to_string(),
            from_hash: "abc".to_string(),
            to_hash: "def".to_string(),
            created_at: DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            updated_at: DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            username: "alice".to_string(),
        };

        let payload = ClPayload::from(&model);
        assert_eq!(payload.status, "open");
        assert_eq!(payload.path, "/repo/path");
    }
}
