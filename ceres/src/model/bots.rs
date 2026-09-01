use callisto::{
    bot_installations,
    sea_orm_active_enums::{InstallationBotStatusEnum, InstallationTargetTypeEnum},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::serde_snowflake::{deserialize_i64_from_string_or_number, serialize_i64_as_string};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct BotRes {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub id: i64,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub bot_id: i64,
    pub target_type: InstallationTargetType,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub target_id: i64,
    pub status: InstallationBotStatus,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub installed_by: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct InstallBotReq {
    pub target_type: InstallationTargetType,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub target_id: i64,
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub installed_by: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ChangeInstallationStatus {
    pub target_type: InstallationTargetType,
    pub status: InstallationBotStatus,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub enum InstallationBotStatus {
    Disabled,
    Enabled,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub enum InstallationTargetType {
    Organization,
    Repository,
}

impl From<bot_installations::Model> for BotRes {
    fn from(value: bot_installations::Model) -> Self {
        Self {
            id: value.id,
            bot_id: value.bot_id,
            target_type: value.target_type.into(),
            target_id: value.target_id,
            status: value.status.into(),
            installed_by: value.installed_by,
        }
    }
}

impl From<InstallationBotStatusEnum> for InstallationBotStatus {
    fn from(value: InstallationBotStatusEnum) -> Self {
        match value {
            InstallationBotStatusEnum::Disabled => InstallationBotStatus::Disabled,
            InstallationBotStatusEnum::Enabled => InstallationBotStatus::Enabled,
        }
    }
}

impl From<InstallationTargetTypeEnum> for InstallationTargetType {
    fn from(value: InstallationTargetTypeEnum) -> Self {
        match value {
            InstallationTargetTypeEnum::Organization => InstallationTargetType::Organization,
            InstallationTargetTypeEnum::Repository => InstallationTargetType::Repository,
        }
    }
}

impl From<InstallationBotStatus> for InstallationBotStatusEnum {
    fn from(value: InstallationBotStatus) -> Self {
        match value {
            InstallationBotStatus::Disabled => InstallationBotStatusEnum::Disabled,
            InstallationBotStatus::Enabled => InstallationBotStatusEnum::Enabled,
        }
    }
}

impl From<InstallationTargetType> for InstallationTargetTypeEnum {
    fn from(value: InstallationTargetType) -> Self {
        match value {
            InstallationTargetType::Organization => InstallationTargetTypeEnum::Organization,
            InstallationTargetType::Repository => InstallationTargetTypeEnum::Repository,
        }
    }
}

/// Authenticated bot principal resolved from a `bot_` bearer token.
#[derive(Debug, Clone)]
pub struct BotIdentity {
    pub bot_id: i64,
    pub token_id: i64,
}

impl BotIdentity {
    pub fn from_models(bot: callisto::bots::Model, token: callisto::bot_tokens::Model) -> Self {
        Self {
            bot_id: bot.id,
            token_id: token.id,
        }
    }
}

/// Request body for creating a new bot token.
#[derive(Deserialize, ToSchema)]
pub struct CreateBotTokenRequest {
    /// Human-readable token name for identification.
    pub token_name: String,
    /// Optional relative expiry in seconds from now.
    pub expires_in: Option<i64>,
}

/// Response body when a bot token is created.
///
/// Note: `token_plain` is only returned once and is never stored in plaintext.
#[derive(Serialize, ToSchema)]
pub struct CreateBotTokenResponse {
    #[serde(serialize_with = "serialize_i64_as_string")]
    #[schema(value_type = String)]
    pub id: i64,
    pub token_name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub token_plain: String,
}

/// Response for mega-init bot bootstrap (`POST /bots/bootstrap-init`).
///
/// Requires header `X-Mega-Init-Secret` matching `MEGA_INIT_BOOTSTRAP_SECRET`.
/// `token` is a `bot_` push token returned once; use as Bearer (or Basic password).
#[derive(Serialize, ToSchema)]
pub struct BootstrapInitBotResponse {
    #[serde(serialize_with = "serialize_i64_as_string")]
    #[schema(value_type = String)]
    pub bot_id: i64,
    pub bot_name: String,
    pub token: String,
}

/// Item in the list bot tokens response.
#[derive(Serialize, ToSchema)]
pub struct ListBotTokenItem {
    #[serde(serialize_with = "serialize_i64_as_string")]
    #[schema(value_type = String)]
    pub id: i64,
    pub token_name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
}
