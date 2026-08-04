use std::ops::Deref;

use callisto::data_backfill_ledger;
use chrono::Utc;
use common::errors::MegaError;
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set, TransactionTrait};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

pub const BACKFILL_ACTOR_CAMPSITE_USER_ID_V1: &str = "actor_campsite_user_id_v1";

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_COMPLETED: &str = "completed";
pub const STATUS_FAILED: &str = "failed";

/// Stale `running` rows older than this may be reclaimed after a crashed replica.
const STALE_RUNNING_MINUTES: i64 = 15;

#[derive(Clone, Debug)]
pub struct MemberIdentityMapping {
    pub campsite_user_id: String,
    pub username: String,
    pub github_login: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DataBackfillStorage {
    pub base: BaseStorage,
}

impl Deref for DataBackfillStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DataBackfillStorage {
    pub async fn get(&self, name: &str) -> Result<Option<data_backfill_ledger::Model>, MegaError> {
        Ok(data_backfill_ledger::Entity::find_by_id(name.to_string())
            .one(self.get_connection())
            .await?)
    }

    /// Try to claim the backfill for this process. Returns `true` if this replica
    /// should run the work. Reclaims stale `running` and retries `failed`/`pending`.
    pub async fn try_claim(&self, name: &str) -> Result<bool, MegaError> {
        let conn = self.get_connection();
        let now = Utc::now().naive_utc();
        let stale_before = (now - chrono::Duration::minutes(STALE_RUNNING_MINUTES))
            .format("%Y-%m-%d %H:%M:%S%.f")
            .to_string();
        let now_s = now.format("%Y-%m-%d %H:%M:%S%.f").to_string();
        let name_esc = esc(name);

        // Ensure row exists (migration seeds it; defensive for older DBs).
        let _ = conn
            .execute_unprepared(&format!(
                r#"
                INSERT INTO data_backfill_ledger (name, status, created_at, updated_at)
                VALUES ('{name_esc}', '{STATUS_PENDING}', '{now_s}', '{now_s}')
                ON CONFLICT (name) DO NOTHING
                "#
            ))
            .await?;

        let result = conn
            .execute_unprepared(&format!(
                r#"
                UPDATE data_backfill_ledger
                   SET status = '{STATUS_RUNNING}', error = NULL, updated_at = '{now_s}'
                 WHERE name = '{name_esc}'
                   AND (
                        status IN ('{STATUS_PENDING}', '{STATUS_FAILED}')
                     OR (status = '{STATUS_RUNNING}' AND updated_at < '{stale_before}')
                   )
                "#
            ))
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_completed(&self, name: &str) -> Result<(), MegaError> {
        let now = Utc::now().naive_utc();
        if let Some(model) = self.get(name).await? {
            let mut am: data_backfill_ledger::ActiveModel = model.into();
            am.status = Set(STATUS_COMPLETED.to_string());
            am.error = Set(None);
            am.updated_at = Set(now);
            am.update(self.get_connection()).await?;
        }
        Ok(())
    }

    pub async fn mark_failed(&self, name: &str, error: &str) -> Result<(), MegaError> {
        let now = Utc::now().naive_utc();
        if let Some(model) = self.get(name).await? {
            let mut am: data_backfill_ledger::ActiveModel = model.into();
            am.status = Set(STATUS_FAILED.to_string());
            am.error = Set(Some(error.chars().take(2000).collect()));
            am.updated_at = Set(now);
            am.update(self.get_connection()).await?;
        }
        Ok(())
    }

    /// Apply handle → campsite_user_id mappings (idempotent UPDATEs).
    pub async fn apply_member_identity_mappings(
        &self,
        mappings: &[MemberIdentityMapping],
    ) -> Result<u64, MegaError> {
        let conn = self.get_connection();
        let mut affected = 0u64;

        let txn = conn.begin().await?;

        for m in mappings {
            let id = m.campsite_user_id.trim();
            if id.is_empty() {
                continue;
            }
            let github = m
                .github_login
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if let Some(login) = github {
                affected += exec_unprepared(
                    &txn,
                    &format!(
                        "UPDATE access_token SET github_login = '{}' WHERE campsite_user_id = '{}' AND (github_login IS NULL OR github_login = '')",
                        esc(login),
                        esc(id)
                    ),
                )
                .await?;
                affected += exec_unprepared(
                    &txn,
                    &format!(
                        "UPDATE mega_cl_reviewer SET github_login = '{}' WHERE campsite_user_id = '{}' AND (github_login IS NULL OR github_login = '')",
                        esc(login),
                        esc(id)
                    ),
                )
                .await?;
            }

            for handle in handles_for(m) {
                if handle == id {
                    continue;
                }
                let h = esc(&handle);
                let i = esc(id);
                let login_sql = github.map(esc).unwrap_or_default();

                for table in [
                    "mega_cl",
                    "mega_issue",
                    "mega_conversation",
                    "reactions",
                    "item_assignees",
                    "mega_code_review_comment",
                    "access_token",
                    "ssh_keys",
                    "cla_sign_status",
                    "user_notification_settings",
                    "user_notification_preferences",
                    "email_jobs",
                    "mega_group_member",
                    "user_approval_status",
                ] {
                    affected += exec_unprepared(
                        &txn,
                        &format!(
                            "UPDATE {table} SET campsite_user_id = '{i}' WHERE campsite_user_id = '{h}'"
                        ),
                    )
                    .await?;
                }

                affected += exec_unprepared(
                    &txn,
                    &format!(
                        r#"
                        DELETE FROM mega_cl_reviewer AS old_row
                         WHERE old_row.campsite_user_id = '{h}'
                           AND EXISTS (
                             SELECT 1 FROM mega_cl_reviewer AS new_row
                              WHERE new_row.cl_link = old_row.cl_link
                                AND new_row.campsite_user_id = '{i}'
                           )
                        "#
                    ),
                )
                .await?;

                affected += exec_unprepared(
                    &txn,
                    &format!(
                        r#"
                        UPDATE mega_cl_reviewer
                           SET campsite_user_id = '{i}'
                             , github_login = COALESCE(NULLIF(github_login, ''), NULLIF('{login_sql}', ''), github_login)
                         WHERE campsite_user_id = '{h}'
                            OR github_login = '{h}'
                        "#
                    ),
                )
                .await?;
            }
        }

        txn.commit().await?;
        Ok(affected)
    }
}

fn handles_for(m: &MemberIdentityMapping) -> Vec<String> {
    let mut out = Vec::new();
    let username = m.username.trim();
    if !username.is_empty() {
        out.push(username.to_string());
    }
    if let Some(g) = m
        .github_login
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && !out.iter().any(|h| h == g)
    {
        out.push(g.to_string());
    }
    out
}

fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

async fn exec_unprepared<C: ConnectionTrait>(conn: &C, sql: &str) -> Result<u64, MegaError> {
    let result = conn
        .execute_unprepared(sql)
        .await
        .map_err(MegaError::from)?;
    Ok(result.rows_affected())
}
