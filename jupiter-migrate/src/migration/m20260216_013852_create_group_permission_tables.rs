use sea_orm::{DatabaseBackend, EnumIter, Iterable, sea_query::extension::postgres::Type};
use sea_orm_migration::{prelude::*, schema::*};

use crate::migration::pk_bigint;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        match backend {
            DatabaseBackend::Postgres => {
                manager
                    .create_type(
                        Type::create()
                            .as_enum(Alias::new("permission_enum"))
                            .values(PermissionEnum::iter())
                            .to_owned(),
                    )
                    .await?;

                manager
                    .create_type(
                        Type::create()
                            .as_enum(Alias::new("resource_type_enum"))
                            .values(ResourceTypeEnum::iter())
                            .to_owned(),
                    )
                    .await?;
            }
            DatabaseBackend::MySql | DatabaseBackend::Sqlite => {}
            _ => {}
        }

        manager
            .create_table(
                Table::create()
                    .table(MegaGroup::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaGroup::Id))
                    .col(string(MegaGroup::Name).unique_key())
                    .col(string_null(MegaGroup::Description))
                    .col(date_time(MegaGroup::CreatedAt))
                    .col(date_time(MegaGroup::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MegaGroupMember::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaGroupMember::Id))
                    .col(big_integer(MegaGroupMember::GroupId))
                    .col(string(MegaGroupMember::Username))
                    .col(date_time(MegaGroupMember::JoinedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_mega_group_member_group_id")
                            .from(MegaGroupMember::Table, MegaGroupMember::GroupId)
                            .to(MegaGroup::Table, MegaGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_group_member_unique")
                    .table(MegaGroupMember::Table)
                    .col(MegaGroupMember::GroupId)
                    .col(MegaGroupMember::Username)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_group_member_username")
                    .table(MegaGroupMember::Table)
                    .col(MegaGroupMember::Username)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MegaResourcePermission::Table)
                    .if_not_exists()
                    .col(pk_bigint(MegaResourcePermission::Id))
                    .col(enumeration(
                        MegaResourcePermission::ResourceType,
                        Alias::new("resource_type_enum"),
                        ResourceTypeEnum::iter(),
                    ))
                    .col(string(MegaResourcePermission::ResourceId))
                    .col(big_integer(MegaResourcePermission::GroupId))
                    .col(enumeration(
                        MegaResourcePermission::Permission,
                        Alias::new("permission_enum"),
                        PermissionEnum::iter(),
                    ))
                    .col(date_time(MegaResourcePermission::CreatedAt))
                    .col(date_time(MegaResourcePermission::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_mega_resource_permission_group_id")
                            .from(
                                MegaResourcePermission::Table,
                                MegaResourcePermission::GroupId,
                            )
                            .to(MegaGroup::Table, MegaGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_resource_permission_unique")
                    .table(MegaResourcePermission::Table)
                    .col(MegaResourcePermission::ResourceType)
                    .col(MegaResourcePermission::ResourceId)
                    .col(MegaResourcePermission::GroupId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_resource_permission_group_id")
                    .table(MegaResourcePermission::Table)
                    .col(MegaResourcePermission::GroupId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(MegaResourcePermission::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(MegaGroupMember::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(MegaGroup::Table).to_owned())
            .await?;

        if manager.get_database_backend() == DatabaseBackend::Postgres {
            manager
                .drop_type(Type::drop().name(Alias::new("permission_enum")).to_owned())
                .await?;
            manager
                .drop_type(
                    Type::drop()
                        .name(Alias::new("resource_type_enum"))
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }
}

#[derive(DeriveIden)]
enum MegaGroup {
    Table,
    Id,
    Name,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MegaGroupMember {
    Table,
    Id,
    GroupId,
    Username,
    JoinedAt,
}

#[derive(DeriveIden)]
enum MegaResourcePermission {
    Table,
    Id,
    ResourceType,
    ResourceId,
    GroupId,
    Permission,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden, EnumIter)]
enum PermissionEnum {
    Read,
    Write,
    Admin,
}

#[derive(Iden, EnumIter)]
enum ResourceTypeEnum {
    Note,
}
