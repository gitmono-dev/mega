use callisto::sea_orm_active_enums::{
    InstallationBotStatusEnum, PermissionEnum, PermissionScopeEnum,
};
use chrono::Utc;
use common::errors::MegaError;
use jupiter::sea_orm::prelude::DateTimeWithTimeZone;

use crate::{
    application::api_service::mono::context::AdminApplicationService,
    model::bots::{
        BotRes, ChangeInstallationStatus, CreateBotTokenResponse, InstallBotReq,
        InstallationTargetType, ListBotTokenItem,
    },
};

impl AdminApplicationService {
    pub async fn get_bot_by_id(
        &self,
        bot_id: i64,
    ) -> Result<Option<callisto::bots::Model>, MegaError> {
        self.ctx
            .storage()
            .bots_storage()
            .get_bot_by_id(bot_id)
            .await
    }

    pub async fn install_bot(&self, bot_id: i64, req: InstallBotReq) -> Result<BotRes, MegaError> {
        let bot = self
            .ctx
            .storage()
            .bots_storage()
            .install_bot(
                bot_id,
                req.target_type.into(),
                req.target_id,
                req.installed_by,
            )
            .await?;
        Ok(bot.into())
    }

    pub async fn list_installed_bots(&self, bot_id: i64) -> Result<Vec<BotRes>, MegaError> {
        Ok(self
            .ctx
            .storage()
            .bots_storage()
            .get_installed_bot_by_id(bot_id)
            .await?
            .into_iter()
            .map(|m| m.into())
            .collect())
    }

    pub async fn change_bot_installation_status(
        &self,
        bot_id: i64,
        installation_id: i64,
        payload: ChangeInstallationStatus,
    ) -> Result<BotRes, MegaError> {
        let model = self
            .ctx
            .storage()
            .bots_storage()
            .change_installed_bot_status(
                bot_id,
                payload.target_type.into(),
                installation_id,
                payload.status.into(),
            )
            .await?;
        Ok(model.into())
    }

    pub async fn uninstall_bot(
        &self,
        bot_id: i64,
        target_type: InstallationTargetType,
        installation_id: i64,
    ) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .bots_storage()
            .uninstall_bot(bot_id, target_type.into(), installation_id)
            .await
    }

    pub async fn generate_bot_token(
        &self,
        bot_id: i64,
        token_name: &str,
        expires_at: Option<DateTimeWithTimeZone>,
    ) -> Result<CreateBotTokenResponse, MegaError> {
        let (model, token_plain) = self
            .ctx
            .storage()
            .bots_storage()
            .generate_bot_token(bot_id, token_name, expires_at)
            .await?;
        Ok(CreateBotTokenResponse {
            id: model.id,
            token_name: model.token_name,
            expires_at: model.expires_at.map(|dt| dt.with_timezone(&Utc)),
            token_plain,
        })
    }

    /// Ensure mega-init bot + fresh push token (for secret-gated bootstrap-init).
    pub async fn ensure_init_bot_token(
        &self,
    ) -> Result<crate::model::bots::BootstrapInitBotResponse, MegaError> {
        let (bot, token_plain) = self
            .ctx
            .storage()
            .bots_storage()
            .ensure_init_bot_token()
            .await?;
        Ok(crate::model::bots::BootstrapInitBotResponse {
            bot_id: bot.id,
            bot_name: bot.name,
            token: token_plain,
        })
    }

    pub async fn list_bot_tokens(&self, bot_id: i64) -> Result<Vec<ListBotTokenItem>, MegaError> {
        Ok(self
            .ctx
            .storage()
            .bots_storage()
            .list_bot_tokens(bot_id)
            .await?
            .into_iter()
            .map(|t| ListBotTokenItem {
                id: t.id,
                token_name: t.token_name,
                expires_at: t.expires_at.map(|dt| dt.with_timezone(&Utc)),
                revoked: t.revoked,
                created_at: t.created_at.with_timezone(&Utc),
            })
            .collect())
    }

    pub async fn revoke_bot_token(&self, bot_id: i64, token_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .bots_storage()
            .revoke_bot_token(bot_id, token_id)
            .await
    }

    pub async fn revoke_all_bot_tokens(&self, bot_id: i64) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .bots_storage()
            .revoke_bot_tokens_by_bot(bot_id)
            .await
    }

    /// Check whether a bot has sufficient permission on a given resource path.
    ///
    /// `resource_id` is treated as a repository path (same as receive-pack).
    /// Requires an Enabled installation covering that path and a permission
    /// scope that satisfies `required_permission`.
    pub async fn check_bot_permission(
        &self,
        bot_id: i64,
        _resource_type: callisto::sea_orm_active_enums::ResourceTypeEnum,
        resource_id: &str,
        required_permission: PermissionEnum,
    ) -> Result<bool, MegaError> {
        let bots_storage = self.ctx.storage().bots_storage();

        let bot = match bots_storage.get_bot_by_id(bot_id).await? {
            Some(b) => b,
            None => return Ok(false),
        };

        if bot.status != callisto::sea_orm_active_enums::BotStatusEnum::Enabled {
            return Ok(false);
        }

        if !scope_satisfies_permission(&bot.permission_scope, &required_permission) {
            return Ok(false);
        }

        // Push-level checks also enforce installation coverage on the path.
        if matches!(
            required_permission,
            PermissionEnum::Write | PermissionEnum::Admin
        ) {
            return bots_storage.bot_may_push_to_path(&bot, resource_id).await;
        }

        // Read: any Enabled installation is enough (scope already checked).
        let installations = bots_storage.get_installed_bot_by_id(bot_id).await?;
        Ok(installations
            .iter()
            .any(|inst| inst.status == InstallationBotStatusEnum::Enabled))
    }
}

fn scope_level(scope: &PermissionScopeEnum) -> u8 {
    match scope {
        PermissionScopeEnum::Read => 1,
        PermissionScopeEnum::Write => 2,
        PermissionScopeEnum::Admin => 3,
    }
}

fn permission_level(permission: &PermissionEnum) -> u8 {
    match permission {
        PermissionEnum::Read => 1,
        PermissionEnum::Write => 2,
        PermissionEnum::Admin => 3,
    }
}

fn scope_satisfies_permission(scope: &PermissionScopeEnum, required: &PermissionEnum) -> bool {
    scope_level(scope) >= permission_level(required)
}
