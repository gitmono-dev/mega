use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // create bots related enum
        let backend = manager.get_database_backend();
        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .create_type(
                        Type::create()
                            .as_enum(BotStatusEnum)
                            .values(BotStatus::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(PermissionScopeEnum)
                            .values(PermissionScope::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(InstallationTargetTypeEnum)
                            .values(InstallationTargetType::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(InstallationBotStatusEnum)
                            .values(InstallationBotStatus::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(TargetTypeEnum)
                            .values(TargetType::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(ActorTypeEnum)
                            .values(ActorType::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(AuditActionEnum)
                            .values(AuditAction::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        // === Bots table ===
        manager
            .create_table(
                Table::create()
                    .table(Bots::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Bots::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Bots::Name).string().not_null())
                    .col(ColumnDef::new(Bots::OrganizationId).big_integer())
                    .col(ColumnDef::new(Bots::CreatorUserId).big_integer().not_null())
                    .col(enumeration(
                        Bots::PermissionScope,
                        Alias::new("permission_scope_enum"),
                        PermissionScope::iter(),
                    ))
                    .col(enumeration(
                        Bots::Status,
                        Alias::new("bot_status_enum"),
                        BotStatus::iter(),
                    ))
                    .col(
                        ColumnDef::new(Bots::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Bots::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // === BotKeys table ===
        manager
            .create_table(
                Table::create()
                    .table(BotKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotKeys::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BotKeys::BotId).big_integer().not_null())
                    .col(ColumnDef::new(BotKeys::PrivateKey).string().not_null())
                    .col(ColumnDef::new(BotKeys::PublicKey).string().not_null())
                    .col(
                        ColumnDef::new(BotKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(BotKeys::Table, BotKeys::BotId)
                            .to(Bots::Table, Bots::Id)
                            .on_delete(sea_query::ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // === BotInstallations table ===
        manager
            .create_table(
                Table::create()
                    .table(BotInstallations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotInstallations::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(BotInstallations::BotId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(enumeration(
                        BotInstallations::TargetType,
                        Alias::new("installation_target_type_enum"),
                        InstallationTargetType::iter(),
                    ))
                    .col(
                        ColumnDef::new(BotInstallations::TargetId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(enumeration(
                        BotInstallations::Status,
                        Alias::new("installation_bot_status_enum"),
                        InstallationBotStatus::iter(),
                    ))
                    .col(
                        ColumnDef::new(BotInstallations::InstalledBy)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BotInstallations::InstalledAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(BotInstallations::Table, BotInstallations::BotId)
                            .to(Bots::Table, Bots::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // === BotTokens table ===
        manager
            .create_table(
                Table::create()
                    .table(BotTokens::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotTokens::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BotTokens::BotId).big_integer().not_null())
                    .col(ColumnDef::new(BotTokens::TokenHash).string().not_null())
                    .col(ColumnDef::new(BotTokens::TokenName).string().not_null())
                    .col(ColumnDef::new(BotTokens::ExpiresAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(BotTokens::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(BotTokens::Revoked)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(BotTokens::Table, BotTokens::BotId)
                            .to(Bots::Table, Bots::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // === AuditLogs table ===
        manager
            .create_table(
                Table::create()
                    .table(AuditLogs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AuditLogs::Id)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AuditLogs::ActorId).big_integer().not_null())
                    .col(enumeration(
                        AuditLogs::ActorType,
                        Alias::new("actor_type_enum"),
                        ActorType::iter(),
                    ))
                    .col(enumeration(
                        AuditLogs::Action,
                        Alias::new("audit_action_enum"),
                        AuditAction::iter(),
                    ))
                    .col(enumeration(
                        AuditLogs::TargetType,
                        Alias::new("target_type_enum"),
                        TargetType::iter(),
                    ))
                    .col(ColumnDef::new(AuditLogs::TargetId).big_integer().not_null())
                    .col(ColumnDef::new(AuditLogs::Metadata).json())
                    .col(
                        ColumnDef::new(AuditLogs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // === Bots table indexes and foreign keys ===
        manager
            .create_index(
                Index::create()
                    .name("uq_bots_name_org")
                    .table(Bots::Table)
                    .col(Bots::Name)
                    .col(Bots::OrganizationId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_bot_keys_bot_id")
                    .table(BotKeys::Table)
                    .col(BotKeys::BotId)
                    .to_owned(),
            )
            .await?;

        // === BotInstallations table indexes and foreign keys ===
        manager
            .create_index(
                Index::create()
                    .name("idx_bot_installations_bot_id")
                    .table(BotInstallations::Table)
                    .col(BotInstallations::BotId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq_bot_installations_target")
                    .table(BotInstallations::Table)
                    .col(BotInstallations::BotId)
                    .col(BotInstallations::TargetType)
                    .col(BotInstallations::TargetId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // === BotTokens table indexes and foreign keys ===
        manager
            .create_index(
                Index::create()
                    .name("idx_bot_tokens_bot_id")
                    .table(BotTokens::Table)
                    .col(BotTokens::BotId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq_bot_tokens_name_hash")
                    .table(BotTokens::Table)
                    .col(BotTokens::BotId)
                    .col(BotTokens::TokenName)
                    .col(BotTokens::TokenHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // === AuditLogs table indexes and foreign keys ===
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_logs_actor")
                    .table(AuditLogs::Table)
                    .col(AuditLogs::ActorId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_audit_logs_target")
                    .table(AuditLogs::Table)
                    .col(AuditLogs::TargetType)
                    .col(AuditLogs::TargetId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Bots {
    Table,
    Id,
    Name,
    OrganizationId,
    CreatorUserId,
    PermissionScope,
    Status,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum BotKeys {
    Table,
    Id,
    BotId,
    PrivateKey,
    PublicKey,
    CreatedAt,
}

#[derive(DeriveIden)]
enum BotInstallations {
    Table,
    Id,
    BotId,
    TargetType,
    TargetId,
    Status,
    InstalledBy,
    InstalledAt,
}

#[derive(DeriveIden)]
enum BotTokens {
    Table,
    Id,
    BotId,
    TokenHash,
    TokenName,
    ExpiresAt,
    CreatedAt,
    Revoked,
}

#[derive(DeriveIden)]
enum AuditLogs {
    Table,
    Id,
    ActorType,
    ActorId,
    Action,
    TargetType,
    TargetId,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
struct BotStatusEnum;
#[derive(Iden, EnumIter)]
pub enum BotStatus {
    Enabled,
    Disabled,
}

#[derive(DeriveIden)]
struct PermissionScopeEnum;
#[derive(Iden, EnumIter)]
pub enum PermissionScope {
    Read,
    Write,
    Admin,
}

#[derive(DeriveIden)]
struct InstallationTargetTypeEnum;
#[derive(Iden, EnumIter)]
pub enum InstallationTargetType {
    Organization,
    Repository,
}

#[derive(DeriveIden)]
struct InstallationBotStatusEnum;
#[derive(Iden, EnumIter)]
pub enum InstallationBotStatus {
    Enabled,
    Disabled,
}

#[derive(DeriveIden)]
struct ActorTypeEnum;
#[derive(Iden, EnumIter)]
pub enum ActorType {
    Human,
    Bot,
}

#[derive(DeriveIden)]
struct AuditActionEnum;

#[derive(Iden, EnumIter)]
pub enum AuditAction {
    Create,
    Update,
    Delete,

    Enable,
    Disable,

    Install,
    Uninstall,

    Generate,
    Revoke,

    Assign,
    Remove,

    Access,
}

#[derive(DeriveIden)]
struct TargetTypeEnum;
#[derive(Iden, EnumIter)]
pub enum TargetType {
    Bot,
    BotInstallation,
    Token,
    Repository,
    Organization,
    User,
}
