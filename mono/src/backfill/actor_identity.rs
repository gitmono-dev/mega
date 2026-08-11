//! Automatic Campsite member identity sync + handle → campsite_user_id backfill.
//!
//! On every mono boot (when `mega_internal_secret` is set):
//! 1. Fetch org=mega `internal/member_identities`
//! 2. UPSERT into local `campsite_member_identity` (for display / alias resolution)
//! 3. Run one-shot actor remapping backfill if not yet completed

use std::time::Instant;

use jupiter::storage::{
    Storage,
    campsite_member_identity_storage::MemberIdentityProfile,
    data_backfill_storage::{BACKFILL_ACTOR_CAMPSITE_USER_ID_V1, MemberIdentityMapping},
};
use serde::Deserialize;
use tracing::{error, info, warn};

const ORG_SLUG: &str = "mega";
const SECRET_HEADER: &str = "X-Mega-Internal-Secret";

#[derive(Debug, Deserialize)]
struct CampsiteMemberIdentity {
    campsite_user_id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    github_login: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Spawn non-blocking identity sync + optional remapping backfill after Storage is ready.
pub fn spawn_actor_identity_backfill(storage: Storage) {
    tokio::spawn(async move {
        if let Err(e) = run_member_identity_sync_and_backfill(storage).await {
            error!(error = %e, "member identity sync/backfill failed (will retry on next boot)");
        }
    });
}

async fn run_member_identity_sync_and_backfill(storage: Storage) -> anyhow::Result<()> {
    let config = storage.config();
    let secret = config.oauth.mega_internal_secret.trim();
    if secret.is_empty() {
        warn!("oauth.mega_internal_secret unset; skipping member identity sync/backfill");
        return Ok(());
    }

    let started = Instant::now();
    let identities = fetch_member_identities(&config.oauth.campsite_api_domain, secret).await?;

    let profiles = identities_to_profiles(&identities);
    let synced = storage
        .campsite_member_identity_storage()
        .upsert_many(&profiles)
        .await?;
    info!(
        synced,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "campsite_member_identity directory synced from Campsite"
    );

    run_actor_remap_backfill_if_needed(&storage, &identities).await?;
    Ok(())
}

async fn run_actor_remap_backfill_if_needed(
    storage: &Storage,
    identities: &[CampsiteMemberIdentity],
) -> anyhow::Result<()> {
    let ledger = storage.data_backfill_storage();
    if let Some(row) = ledger.get(BACKFILL_ACTOR_CAMPSITE_USER_ID_V1).await?
        && row.status == "completed"
    {
        info!(
            backfill = BACKFILL_ACTOR_CAMPSITE_USER_ID_V1,
            "actor identity backfill already completed; skip"
        );
        return Ok(());
    }

    if !ledger.try_claim(BACKFILL_ACTOR_CAMPSITE_USER_ID_V1).await? {
        info!(
            backfill = BACKFILL_ACTOR_CAMPSITE_USER_ID_V1,
            "actor identity backfill claimed by another replica or already done; skip"
        );
        return Ok(());
    }

    let started = Instant::now();
    let mappings: Vec<MemberIdentityMapping> = identities
        .iter()
        .filter(|i| !i.campsite_user_id.trim().is_empty())
        .map(|i| MemberIdentityMapping {
            campsite_user_id: i.campsite_user_id.trim().to_string(),
            username: i.username.clone(),
            github_login: i
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
        })
        .collect();

    match storage
        .data_backfill_storage()
        .apply_member_identity_mappings(&mappings)
        .await
    {
        Ok(affected) => {
            ledger
                .mark_completed(BACKFILL_ACTOR_CAMPSITE_USER_ID_V1)
                .await?;
            info!(
                backfill = BACKFILL_ACTOR_CAMPSITE_USER_ID_V1,
                mappings = mappings.len(),
                rows_affected = affected,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "actor identity backfill completed"
            );
            Ok(())
        }
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = ledger
                .mark_failed(BACKFILL_ACTOR_CAMPSITE_USER_ID_V1, &msg)
                .await;
            Err(e.into())
        }
    }
}

async fn fetch_member_identities(
    api_base: &str,
    secret: &str,
) -> anyhow::Result<Vec<CampsiteMemberIdentity>> {
    let url = format!(
        "{}/v1/organizations/{}/internal/member_identities",
        api_base.trim_end_matches('/'),
        ORG_SLUG
    );

    let client = reqwest::Client::builder().no_proxy().build()?;
    let resp = client
        .get(&url)
        .header(SECRET_HEADER, secret)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("campsite member_identities request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("campsite member_identities HTTP {status}: {body}");
    }

    resp.json()
        .await
        .map_err(|e| anyhow::anyhow!("parse member_identities JSON: {e}"))
}

fn identities_to_profiles(identities: &[CampsiteMemberIdentity]) -> Vec<MemberIdentityProfile> {
    identities
        .iter()
        .filter(|i| !i.campsite_user_id.trim().is_empty())
        .map(|i| {
            let github_login = i
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let username = i.username.trim().to_string();
            MemberIdentityProfile {
                campsite_user_id: i.campsite_user_id.trim().to_string(),
                username: if !username.is_empty() {
                    username
                } else {
                    github_login.clone().unwrap_or_default()
                },
                github_login,
                display_name: i
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("")
                    .to_string(),
                email: i
                    .email
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect()
}
