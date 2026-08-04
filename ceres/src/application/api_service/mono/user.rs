use common::errors::MegaError;

use super::context::UserApplicationService;
use crate::model::{
    notification::{
        NotificationEventTypeInfo, UpdateUserNotificationConfig, UserNotificationConfig,
        UserNotificationPreferenceItem,
    },
    user::{ListSSHKey, ListToken},
};

impl UserApplicationService {
    pub async fn save_ssh_key(
        &self,
        username: String,
        title: &str,
        ssh_key: &str,
        fingerprint: &str,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .user_storage()
            .save_ssh_key(username, title, ssh_key, fingerprint)
            .await
    }

    pub async fn delete_ssh_key(&self, username: String, key_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .user_storage()
            .delete_ssh_key(username, key_id)
            .await
    }

    pub async fn list_user_ssh_keys(&self, username: String) -> Result<Vec<ListSSHKey>, MegaError> {
        let keys = self
            .ctx
            .storage()
            .user_storage()
            .list_user_ssh(username)
            .await?;
        Ok(keys.into_iter().map(|k| k.into()).collect())
    }

    pub async fn generate_user_token(
        &self,
        campsite_user_id: String,
        github_login: Option<String>,
    ) -> Result<String, MegaError> {
        self.ctx
            .storage()
            .user_storage()
            .generate_token(campsite_user_id, github_login)
            .await
    }

    pub async fn delete_user_token(&self, username: String, key_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .user_storage()
            .delete_token(username, key_id)
            .await
    }

    pub async fn list_user_tokens(&self, username: String) -> Result<Vec<ListToken>, MegaError> {
        let tokens = self
            .ctx
            .storage()
            .user_storage()
            .list_token(username)
            .await?;
        Ok(tokens.into_iter().map(|t| t.into()).collect())
    }

    pub async fn list_notification_event_types(
        &self,
    ) -> Result<Vec<NotificationEventTypeInfo>, MegaError> {
        Ok(self
            .ctx
            .storage()
            .notification_storage()
            .list_event_types()
            .await?
            .into_iter()
            .map(|t| NotificationEventTypeInfo {
                code: t.code,
                category: t.category,
                description: t.description,
                system_required: t.system_required,
                default_enabled: t.default_enabled,
            })
            .collect())
    }

    pub async fn get_user_notification_config(
        &self,
        username: &str,
        email: &str,
    ) -> Result<UserNotificationConfig, MegaError> {
        let stg = self.ctx.storage().notification_storage();
        stg.upsert_user_settings(username, email).await?;

        let settings = stg
            .get_user_settings(username)
            .await?
            .ok_or_else(|| MegaError::Other("user settings missing".to_string()))?;

        let prefs = stg
            .list_user_preferences(username)
            .await?
            .into_iter()
            .map(|p| UserNotificationPreferenceItem {
                event_type_code: p.event_type_code,
                enabled: p.enabled,
            })
            .collect();

        Ok(UserNotificationConfig {
            enabled: settings.enabled,
            delivery_mode: settings.delivery_mode,
            email: settings.email,
            preferences: prefs,
        })
    }

    pub async fn update_user_notification_config(
        &self,
        username: &str,
        email: &str,
        payload: UpdateUserNotificationConfig,
    ) -> Result<(), MegaError> {
        let stg = self.ctx.storage().notification_storage();
        stg.upsert_user_settings(username, email).await?;

        if let Some(enabled) = payload.enabled {
            stg.set_global_enabled(username, enabled).await?;
        }
        if let Some(mode) = payload.delivery_mode {
            stg.set_delivery_mode(username, &mode).await?;
        }
        if let Some(items) = payload.preferences {
            for item in items {
                stg.set_user_preference(username, &item.event_type_code, item.enabled)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn find_bot_by_token(
        &self,
        token: &str,
    ) -> Result<Option<(callisto::bots::Model, callisto::bot_tokens::Model)>, MegaError> {
        self.ctx
            .storage()
            .bots_storage()
            .find_bot_by_token(token)
            .await
    }
}
