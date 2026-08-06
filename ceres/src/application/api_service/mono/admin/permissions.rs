//! Global admin permission operations.
//!
//! Effective admins are the **union** of:
//! - `monorepo.admin` in mega config (always applied; used at monorepo init and at runtime)
//! - users in the root `/.mega_cedar.json` admin group
//!
//! # Design
//! - Config admins are checked on every request (not baked into Redis), so they
//!   remain valid even when Cedar/Redis is stale or missing
//! - Cedar-derived admins are Redis-cached (TTL 10 minutes) to avoid re-parsing
//!   `.mega_cedar.json`

use std::collections::BTreeSet;

use common::errors::MegaError;
use git_internal::internal::object::tree::Tree;
use jupiter::{redis::AsyncCommands, utils::converter::FromMegaModel};

use crate::application::api_service::mono::context::AdminApplicationService;

/// Cache TTL for Cedar admin list (10 minutes).
pub const ADMIN_CACHE_TTL: u64 = 600;

/// The Cedar entity file name in root directory.
pub const ADMIN_FILE: &str = ".mega_cedar.json";

/// Redis cache key suffix for Cedar admin list (config admins are merged at read time).
const ADMIN_CACHE_KEY_SUFFIX: &str = "admin:list";

impl AdminApplicationService {
    /// Check if a user is an admin (config `monorepo.admin` or Cedar).
    pub async fn check_is_admin(&self, username: &str) -> Result<bool, MegaError> {
        let username = username.trim();
        if username.is_empty() {
            return Ok(false);
        }
        let admins = self.get_effective_admins().await?;
        Ok(admins.iter().any(|a| a == username))
    }

    /// Retrieve all effective admin identities (config ∪ Cedar), sorted uniquely.
    pub async fn get_all_admins(&self) -> Result<Vec<String>, MegaError> {
        self.get_effective_admins().await
    }

    /// GitHub logins (or Cedar euids) listed under `[monorepo] admin` in config.
    fn config_admins(&self) -> Vec<String> {
        self.ctx
            .storage()
            .config()
            .monorepo
            .admin
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Merge Cedar admins with config admins (sorted, unique).
    fn merge_with_config_admins(&self, cedar_admins: Vec<String>) -> Vec<String> {
        let mut set: BTreeSet<String> = cedar_admins.into_iter().collect();
        for admin in self.config_admins() {
            set.insert(admin);
        }
        set.into_iter().collect()
    }

    /// Get effective admins: Redis/Cedar list ∪ `monorepo.admin`.
    ///
    /// Config admins are always merged after cache/file load so a stale Redis
    /// Cedar list cannot drop configured admins. If `.mega_cedar.json` is
    /// missing, config admins alone still apply.
    async fn get_effective_admins(&self) -> Result<Vec<String>, MegaError> {
        let cedar_admins = self.get_cedar_admins().await?;
        Ok(self.merge_with_config_admins(cedar_admins))
    }

    /// Cedar-only admin list (cached). Does not include config admins.
    async fn get_cedar_admins(&self) -> Result<Vec<String>, MegaError> {
        if let Ok(admins) = self.get_admins_from_cache().await {
            return Ok(admins);
        }

        let store = match self.load_admin_entity_store().await {
            Ok(store) => store,
            Err(e) if is_admin_config_unavailable(&e) => {
                tracing::warn!(
                    error = %e,
                    "Admin Cedar config unavailable; using monorepo.admin from config only"
                );
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        let resolver = saturn::admin_resolver::AdminResolver::from_entity_store(&store);
        let admins = resolver.admin_list();

        if let Err(e) = self.cache_admins(&admins).await {
            tracing::warn!("Failed to write admin cache: {}", e);
        }

        Ok(admins)
    }

    /// Invalidate the Cedar admin list cache.
    /// This should be called when the `.mega_cedar.json` file is modified.
    pub async fn invalidate_admin_cache(&self) {
        let mut conn = self.ctx.git_object_cache().connection.clone();
        let key = format!(
            "{}:{}",
            self.ctx.git_object_cache().prefix,
            ADMIN_CACHE_KEY_SUFFIX
        );
        if let Err(e) = conn.del::<_, ()>(&key).await {
            tracing::warn!("Failed to invalidate admin cache: {}", e);
        }
    }

    /// Load EntityStore from `/.mega_cedar.json`.
    async fn load_admin_entity_store(&self) -> Result<saturn::entitystore::EntityStore, MegaError> {
        let mono_storage = self.ctx.storage().mono_storage();

        let root_ref = mono_storage
            .get_main_ref("/")
            .await?
            .ok_or_else(|| MegaError::Other("Root ref not found".into()))?;

        let root_tree = Tree::from_mega_model(
            mono_storage
                .get_tree_by_hash(&root_ref.ref_tree_hash)
                .await?
                .ok_or_else(|| MegaError::Other("Root tree not found".into()))?,
        );

        let blob_item = root_tree
            .tree_items
            .iter()
            .find(|item| item.name == ADMIN_FILE)
            .ok_or_else(|| {
                MegaError::Other(format!("{} not found in root directory", ADMIN_FILE))
            })?;

        let blob_hash = blob_item.id.to_string();
        let content_bytes = match self
            .ctx
            .storage()
            .git_service
            .get_object_as_bytes(&blob_hash)
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                // Best-effort classification/logging for ObjStorageNotFound cases.
                let e = self
                    .ctx
                    .storage()
                    .classify_blob_objstorage_not_found(&blob_hash, e)
                    .await;
                return Err(e);
            }
        };

        let content = String::from_utf8(content_bytes)
            .map_err(|e| MegaError::Other(format!("UTF-8 decode failed: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| MegaError::Other(format!("JSON parse failed: {}", e)))
    }

    async fn get_admins_from_cache(&self) -> Result<Vec<String>, MegaError> {
        let mut conn = self.ctx.git_object_cache().connection.clone();
        let key = format!(
            "{}:{}",
            self.ctx.git_object_cache().prefix,
            ADMIN_CACHE_KEY_SUFFIX
        );
        let data: Option<String> = conn.get(&key).await?;

        match data {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| MegaError::Other(format!("Parse cache failed: {}", e))),
            None => Err(MegaError::Other("Cache miss".into())),
        }
    }

    async fn cache_admins(&self, admins: &[String]) -> Result<(), MegaError> {
        let mut conn = self.ctx.git_object_cache().connection.clone();
        let json = serde_json::to_string(admins)
            .map_err(|e| MegaError::Other(format!("Serialize failed: {}", e)))?;

        let key = format!(
            "{}:{}",
            self.ctx.git_object_cache().prefix,
            ADMIN_CACHE_KEY_SUFFIX
        );
        conn.set_ex::<_, _, ()>(&key, json, ADMIN_CACHE_TTL).await?;
        Ok(())
    }
}

fn is_admin_config_unavailable(err: &MegaError) -> bool {
    let msg = err.to_string();
    msg.contains(".mega_cedar.json not found")
        || msg.contains("Root ref not found")
        || msg.contains("Root tree not found")
}
