//! Automatic handle → campsite_user_id backfill on mono startup.
//!
//! Fetches org=mega member identities from Campsite
//! `GET /v1/organizations/mega/internal/member_identities` using
//! `X-Mega-Internal-Secret`, then applies SQL updates via
//! [`jupiter::storage::data_backfill_storage`].

use std::time::Instant;

use jupiter::storage::{
    Storage,
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
}

/// Spawn non-blocking backfill after Storage is ready. Failures are logged;
/// ledger stays `failed`/`pending` so the next boot retries.
pub fn spawn_actor_identity_backfill(storage: Storage) {
    tokio::spawn(async move {
        if let Err(e) = run_actor_identity_backfill(storage).await {
            error!(error = %e, "actor identity backfill failed (will retry on next boot)");
        }
    });
}

async fn run_actor_identity_backfill(storage: Storage) -> anyhow::Result<()> {
    let config = storage.config();
    let secret = config.oauth.mega_internal_secret.trim();
    if secret.is_empty() {
        warn!("oauth.mega_internal_secret unset; skipping automatic actor identity backfill");
        return Ok(());
    }

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
    match fetch_and_apply(&storage, &config.oauth.campsite_api_domain, secret).await {
        Ok((mappings, affected)) => {
            ledger
                .mark_completed(BACKFILL_ACTOR_CAMPSITE_USER_ID_V1)
                .await?;
            info!(
                backfill = BACKFILL_ACTOR_CAMPSITE_USER_ID_V1,
                mappings,
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
            Err(e)
        }
    }
}

async fn fetch_and_apply(
    storage: &Storage,
    api_base: &str,
    secret: &str,
) -> anyhow::Result<(usize, u64)> {
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

    let identities: Vec<CampsiteMemberIdentity> = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("parse member_identities JSON: {e}"))?;

    let mappings: Vec<MemberIdentityMapping> = identities
        .into_iter()
        .filter(|i| !i.campsite_user_id.trim().is_empty())
        .map(|i| MemberIdentityMapping {
            campsite_user_id: i.campsite_user_id.trim().to_string(),
            username: i.username,
            github_login: i
                .github_login
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        })
        .collect();

    let count = mappings.len();
    let affected = storage
        .data_backfill_storage()
        .apply_member_identity_mappings(&mappings)
        .await?;
    Ok((count, affected))
}
