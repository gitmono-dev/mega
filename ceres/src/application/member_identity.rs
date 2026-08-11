//! Campsite member identity helpers for transitional actor strings
//! (campsite username / github_login ↔ campsite public id).
//!
//! Display / alias resolution reads the local `campsite_member_identity` table.
//! Campsite HTTP is only used for startup sync / backfill write paths.

use std::collections::{HashMap, HashSet};

use common::errors::MegaError;
use jupiter::storage::{Storage, campsite_member_identity_storage::MemberIdentityProfile};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct CampsiteMemberIdentity {
    pub campsite_user_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub github_login: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// Fetch org=mega member identities when `mega_internal_secret` is configured.
/// Used by startup sync / backfill — not by per-request display resolution.
pub async fn fetch_campsite_member_identities(
    storage: &Storage,
) -> Result<Vec<CampsiteMemberIdentity>, MegaError> {
    let config = storage.config();
    let secret = config.oauth.mega_internal_secret.trim();
    let api_base = config.oauth.campsite_api_domain.trim();
    if secret.is_empty() || api_base.is_empty() {
        return Ok(Vec::new());
    }

    let url = format!(
        "{}/v1/organizations/mega/internal/member_identities",
        api_base.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| MegaError::Other(e.to_string()))?;
    let resp = client
        .get(&url)
        .header("X-Mega-Internal-Secret", secret)
        .send()
        .await
        .map_err(|e| MegaError::Other(format!("campsite member_identities request failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(MegaError::Other(format!(
            "campsite member_identities HTTP {status}: {body}"
        )));
    }
    resp.json()
        .await
        .map_err(|e| MegaError::Other(format!("parse member_identities JSON: {e}")))
}

fn push_unique(out: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    let v = value.trim();
    if v.is_empty() || !seen.insert(v.to_string()) {
        return;
    }
    out.push(v.to_string());
}

fn identity_handles_from_row(
    campsite_user_id: &str,
    username: &str,
    github_login: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut out, &mut seen, campsite_user_id);
    push_unique(&mut out, &mut seen, username);
    if let Some(login) = github_login {
        push_unique(&mut out, &mut seen, login);
    }
    out
}

/// Upsert a local identity row (write-through from login / approve / sync).
pub async fn upsert_local_identity(
    storage: &Storage,
    profile: MemberIdentityProfile,
) -> Result<(), MegaError> {
    storage
        .campsite_member_identity_storage()
        .upsert(profile)
        .await
}

/// Persist Campsite API identities into the local directory table.
pub async fn sync_identities_to_local(
    storage: &Storage,
    identities: &[CampsiteMemberIdentity],
) -> Result<u64, MegaError> {
    let profiles: Vec<MemberIdentityProfile> = identities
        .iter()
        .filter(|i| !i.campsite_user_id.trim().is_empty())
        .map(|i| {
            let username = i.username.trim().to_string();
            let github_login = i
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let display_name = i
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            let email = i
                .email
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            MemberIdentityProfile {
                campsite_user_id: i.campsite_user_id.trim().to_string(),
                username: if !username.is_empty() {
                    username
                } else {
                    github_login.clone().unwrap_or_default()
                },
                github_login,
                display_name,
                email,
            }
        })
        .collect();
    storage
        .campsite_member_identity_storage()
        .upsert_many(&profiles)
        .await
}

/// Resolve all known actor strings for the same person as `actor`.
/// Always includes `actor` itself; expands via local identity table + github_login maps.
pub async fn aliases_for_actor(storage: &Storage, actor: &str) -> Vec<String> {
    let actor = actor.trim();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut out, &mut seen, actor);
    if actor.is_empty() {
        return out;
    }

    match storage
        .campsite_member_identity_storage()
        .find_by_actor(actor)
        .await
    {
        Ok(Some(row)) => {
            for handle in identity_handles_from_row(
                &row.campsite_user_id,
                &row.username,
                row.github_login.as_deref(),
            ) {
                push_unique(&mut out, &mut seen, &handle);
            }
        }
        Ok(None) => {}
        Err(e) => {
            tracing::debug!(error = %e, actor, "local member identity lookup failed for CLA aliases")
        }
    }

    match storage.user_storage().github_login_to_campsite_ids().await {
        Ok(map) => extend_from_login_map(&mut out, &mut seen, actor, &map),
        Err(e) => {
            tracing::debug!(error = %e, "failed to load github_login map for CLA aliases")
        }
    }
    match storage
        .reviewer_storage()
        .github_login_to_campsite_ids()
        .await
    {
        Ok(map) => extend_from_login_map(&mut out, &mut seen, actor, &map),
        Err(e) => {
            tracing::debug!(error = %e, "failed to load reviewer github_login map for CLA aliases")
        }
    }

    out
}

fn extend_from_login_map(
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
    actor: &str,
    map: &HashMap<String, String>,
) {
    if let Some(id) = map.get(actor) {
        push_unique(out, seen, id);
    }
    for (login, id) in map {
        if id.eq_ignore_ascii_case(actor) || login.eq_ignore_ascii_case(actor) {
            push_unique(out, seen, login);
            push_unique(out, seen, id);
        }
    }
}

/// Resolve a stored actor id (usually campsite public id) to a human-readable label
/// for UI surfaces such as account-review "reviewed by".
///
/// Preference order: local username → github_login → display_name → approval display_name → raw actor.
pub async fn display_label_for_actor(storage: &Storage, actor: &str) -> String {
    let labels = display_labels_for_actors(storage, &[actor.to_string()]).await;
    labels
        .get(actor.trim())
        .cloned()
        .unwrap_or_else(|| actor.trim().to_string())
}

/// Batch variant of [`display_label_for_actor`] — reads only the local identity table
/// (plus local approval / github_login fallbacks). Does **not** call Campsite.
pub async fn display_labels_for_actors(
    storage: &Storage,
    actors: &[String],
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut needed: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for actor in actors {
        let actor = actor.trim();
        if actor.is_empty() || !seen.insert(actor.to_string()) {
            continue;
        }
        needed.push(actor.to_string());
    }
    if needed.is_empty() {
        return out;
    }

    let rows = storage
        .campsite_member_identity_storage()
        .get_by_ids(&needed)
        .await
        .unwrap_or_default();
    let mut by_id: HashMap<String, callisto::campsite_member_identity::Model> = HashMap::new();
    let mut by_handle: HashMap<String, callisto::campsite_member_identity::Model> = HashMap::new();
    for row in rows {
        by_handle.insert(row.campsite_user_id.to_ascii_lowercase(), row.clone());
        if !row.username.trim().is_empty() {
            by_handle.insert(row.username.to_ascii_lowercase(), row.clone());
        }
        if let Some(login) = row
            .github_login
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            by_handle.insert(login.to_ascii_lowercase(), row.clone());
        }
        by_id.insert(row.campsite_user_id.clone(), row);
    }

    // Also resolve actors that matched username/github_login but weren't in the id list.
    for actor in &needed {
        if by_id.contains_key(actor) || by_handle.contains_key(&actor.to_ascii_lowercase()) {
            continue;
        }
        if let Ok(Some(row)) = storage
            .campsite_member_identity_storage()
            .find_by_actor(actor)
            .await
        {
            by_handle.insert(actor.to_ascii_lowercase(), row.clone());
            by_id.insert(row.campsite_user_id.clone(), row);
        }
    }

    let login_map = storage
        .user_storage()
        .github_login_to_campsite_ids()
        .await
        .unwrap_or_default();

    for actor in &needed {
        if let Some(row) = by_id
            .get(actor)
            .or_else(|| by_handle.get(&actor.to_ascii_lowercase()))
        {
            let username = row.username.trim();
            if !username.is_empty() {
                out.insert(actor.clone(), username.to_string());
                continue;
            }
            if let Some(login) = row
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                out.insert(actor.clone(), login.to_string());
                continue;
            }
            let display = row.display_name.trim();
            if !display.is_empty() {
                out.insert(actor.clone(), display.to_string());
                continue;
            }
        }

        if let Ok(Some(row)) = storage.user_approval_storage().get(actor).await {
            let name = row.display_name.trim();
            if !name.is_empty() {
                out.insert(actor.clone(), name.to_string());
                continue;
            }
        }

        if let Some(login) = login_map
            .iter()
            .find_map(|(login, id)| id.eq_ignore_ascii_case(actor).then_some(login.clone()))
        {
            out.insert(actor.clone(), login);
            continue;
        }

        out.insert(actor.clone(), actor.clone());
    }

    out
}
