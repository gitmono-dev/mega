use api_model::common::Pagination;
use callisto::{
    mega_group, mega_group_member, mega_resource_permission,
    sea_orm_active_enums::{PermissionEnum, ResourceTypeEnum},
};
use common::errors::MegaError;
use jupiter::model::group_dto::{DeleteGroupStats, ResourcePermissionBinding};

use crate::{
    application::api_service::mono::context::AdminApplicationService,
    model::group::{
        CreateGroupRequest, PermissionBindingRequest, PermissionValue, ResourceTypeValue,
        UpdateGroupRequest,
    },
};

fn normalize_optional_description(description: Option<String>) -> Option<String> {
    description
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn validate_group_name(name: &str) -> Result<String, MegaError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(MegaError::bad_request("Group name must not be empty"));
    }
    if name.len() > 255 {
        return Err(MegaError::bad_request(
            "Group name must not exceed 255 characters",
        ));
    }
    Ok(name.to_string())
}

fn validate_pagination(pagination: &Pagination) -> Result<(), MegaError> {
    if pagination.page == 0 {
        return Err(MegaError::bad_request("pagination.page must be >= 1"));
    }
    if pagination.per_page == 0 {
        return Err(MegaError::bad_request("pagination.per_page must be >= 1"));
    }
    Ok(())
}

fn create_group_payload(req: CreateGroupRequest) -> jupiter::model::group_dto::CreateGroupPayload {
    jupiter::model::group_dto::CreateGroupPayload {
        name: req.name,
        description: req.description,
    }
}

fn update_group_payload(req: UpdateGroupRequest) -> jupiter::model::group_dto::UpdateGroupPayload {
    jupiter::model::group_dto::UpdateGroupPayload {
        name: req.name,
        description: req.description,
    }
}

fn permission_bindings(
    permissions: Vec<PermissionBindingRequest>,
) -> Vec<ResourcePermissionBinding> {
    permissions
        .into_iter()
        .map(|item| ResourcePermissionBinding {
            group_id: item.group_id,
            permission: item.permission.into(),
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct EffectiveResourcePermission {
    pub is_admin: bool,
    pub permission: Option<PermissionEnum>,
}

impl AdminApplicationService {
    pub async fn create_group(
        &self,
        payload: CreateGroupRequest,
    ) -> Result<mega_group::Model, MegaError> {
        let name = validate_group_name(&payload.name)?;
        let description = normalize_optional_description(payload.description);

        self.ctx
            .storage()
            .group_storage()
            .create_group(create_group_payload(CreateGroupRequest {
                name,
                description,
            }))
            .await
    }

    pub async fn list_groups(
        &self,
        page: Pagination,
    ) -> Result<(Vec<mega_group::Model>, u64), MegaError> {
        validate_pagination(&page)?;
        self.ctx.storage().group_storage().list_groups(page).await
    }

    pub async fn get_group_by_id(
        &self,
        group_id: i64,
    ) -> Result<Option<mega_group::Model>, MegaError> {
        self.ctx
            .storage()
            .group_storage()
            .get_group_by_id(group_id)
            .await
    }

    pub async fn update_group(
        &self,
        group_id: i64,
        payload: UpdateGroupRequest,
    ) -> Result<mega_group::Model, MegaError> {
        let name = validate_group_name(&payload.name)?;
        let description = normalize_optional_description(payload.description);
        self.ctx
            .storage()
            .group_storage()
            .update_group(
                group_id,
                update_group_payload(UpdateGroupRequest { name, description }),
            )
            .await
    }

    pub async fn delete_group(&self, group_id: i64) -> Result<DeleteGroupStats, MegaError> {
        let stats = self
            .ctx
            .storage()
            .group_storage()
            .delete_group_with_relations(group_id)
            .await?;

        if stats.deleted_groups_count == 0 {
            return Err(MegaError::NotFound(format!(
                "Group not found: {}",
                group_id
            )));
        }

        Ok(stats)
    }

    pub async fn add_group_members(
        &self,
        group_id: i64,
        usernames: Vec<String>,
    ) -> Result<Vec<mega_group_member::Model>, MegaError> {
        if usernames.is_empty() {
            return Err(MegaError::bad_request("usernames must not be empty"));
        }
        let group_storage = self.ctx.storage().group_storage();
        group_storage.add_group_members(group_id, &usernames).await
    }

    pub async fn remove_group_member(
        &self,
        group_id: i64,
        username: &str,
    ) -> Result<bool, MegaError> {
        let group_storage = self.ctx.storage().group_storage();
        if group_storage.get_group_by_id(group_id).await?.is_none() {
            return Err(MegaError::NotFound(format!(
                "Group not found: {}",
                group_id
            )));
        }
        group_storage.remove_group_member(group_id, username).await
    }

    pub async fn list_group_members(
        &self,
        group_id: i64,
        page: Pagination,
    ) -> Result<(Vec<mega_group_member::Model>, u64), MegaError> {
        validate_pagination(&page)?;
        let group_storage = self.ctx.storage().group_storage();
        if group_storage.get_group_by_id(group_id).await?.is_none() {
            return Err(MegaError::NotFound(format!(
                "Group not found: {}",
                group_id
            )));
        }
        group_storage.list_group_members(group_id, page).await
    }

    pub async fn set_resource_permission(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        permissions: Vec<PermissionBindingRequest>,
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        let bindings = permission_bindings(permissions);
        self.ctx
            .storage()
            .group_storage()
            .replace_resource_permissions(resource_type, resource_id, &bindings)
            .await
    }

    pub async fn get_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        self.ctx
            .storage()
            .group_storage()
            .list_resource_permissions(resource_type, resource_id)
            .await
    }

    pub async fn update_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        permissions: Vec<PermissionBindingRequest>,
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        let bindings = permission_bindings(permissions);
        self.ctx
            .storage()
            .group_storage()
            .upsert_resource_permissions(resource_type, resource_id, &bindings)
            .await
    }

    pub async fn delete_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
    ) -> Result<u64, MegaError> {
        self.ctx
            .storage()
            .group_storage()
            .delete_resource_permissions(resource_type, resource_id)
            .await
    }

    pub async fn get_user_groups(
        &self,
        username: &str,
    ) -> Result<Vec<mega_group::Model>, MegaError> {
        self.ctx
            .storage()
            .group_storage()
            .find_groups_by_username(username)
            .await
    }

    pub async fn get_user_effective_permission(
        &self,
        username: &str,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
    ) -> Result<EffectiveResourcePermission, MegaError> {
        if self.check_is_admin(username).await? {
            return Ok(EffectiveResourcePermission {
                is_admin: true,
                permission: Some(PermissionEnum::Admin),
            });
        }

        let group_storage = self.ctx.storage().group_storage();
        let group_ids = group_storage
            .find_group_ids_by_username(username)
            .await
            .map_err(|_| MegaError::Other("Failed to query user group memberships".to_string()))?;
        if group_ids.is_empty() {
            return Ok(EffectiveResourcePermission {
                is_admin: false,
                permission: None,
            });
        }

        let permissions = group_storage
            .find_permissions_by_resource(resource_type, resource_id, &group_ids)
            .await
            .map_err(|_| MegaError::Other("Failed to query resource permissions".to_string()))?;

        let permission = permissions
            .iter()
            .map(|item| item.permission.clone())
            .max_by_key(permission_level);

        Ok(EffectiveResourcePermission {
            is_admin: false,
            permission,
        })
    }

    pub async fn resolve_resource_id(
        &self,
        resource_type: ResourceTypeValue,
        resource_id: &str,
    ) -> Result<String, MegaError> {
        let normalized_resource_id = resource_id.trim();
        if normalized_resource_id.is_empty() {
            return Err(MegaError::Other(
                "resource_id must not be empty".to_string(),
            ));
        }

        match resource_type {
            ResourceTypeValue::Note => {
                let note = self
                    .ctx
                    .storage()
                    .note_storage()
                    .get_note_by_public_id(normalized_resource_id)
                    .await?;
                match note {
                    Some(note) => Ok(note.public_id),
                    None => {
                        tracing::warn!(
                            resource_id = normalized_resource_id,
                            "note resource missing in mono notes table; falling back to raw public_id"
                        );
                        Ok(normalized_resource_id.to_string())
                    }
                }
            }
        }
    }

    /// Validates resource type and resolves the canonical resource id (e.g. note public_id).
    pub async fn resolve_resource_context(
        &self,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(ResourceTypeValue, String), MegaError> {
        let resource_type_value = ResourceTypeValue::try_from(resource_type)
            .map_err(|err| MegaError::Other(err.to_string()))?;
        let validated_resource_id = self
            .resolve_resource_id(resource_type_value, resource_id)
            .await?;
        Ok((resource_type_value, validated_resource_id))
    }

    /// Reserved for future business-route authorization integration.
    pub async fn check_resource_permission(
        &self,
        username: &str,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        required_permission: PermissionEnum,
    ) -> Result<bool, MegaError> {
        let effective = self
            .get_user_effective_permission(username, resource_type, resource_id)
            .await?;

        if effective.is_admin {
            return Ok(true);
        }

        Ok(match effective.permission {
            Some(permission) => permission_satisfies(&permission, &required_permission),
            None => false,
        })
    }

    pub async fn check_resource_permission_value(
        &self,
        username: &str,
        resource_type: ResourceTypeValue,
        resource_id: &str,
        required_permission: PermissionValue,
    ) -> Result<bool, MegaError> {
        self.check_resource_permission(
            username,
            resource_type.into(),
            resource_id,
            required_permission.into(),
        )
        .await
    }
}

fn permission_level(permission: &PermissionEnum) -> u8 {
    match permission {
        PermissionEnum::Read => 1,
        PermissionEnum::Write => 2,
        PermissionEnum::Admin => 3,
    }
}

fn permission_satisfies(current: &PermissionEnum, required: &PermissionEnum) -> bool {
    permission_level(current) >= permission_level(required)
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::PermissionEnum;

    use super::{create_group_payload, permission_bindings, update_group_payload};
    use crate::model::group::{
        CreateGroupRequest, PermissionBindingRequest, PermissionValue, UpdateGroupRequest,
    };

    #[test]
    fn create_group_payload_maps_request_fields() {
        let payload = create_group_payload(CreateGroupRequest {
            name: "admins".to_string(),
            description: Some("core".to_string()),
        });
        assert_eq!(payload.name, "admins");
        assert_eq!(payload.description.as_deref(), Some("core"));
    }

    #[test]
    fn update_group_payload_maps_request_fields() {
        let payload = update_group_payload(UpdateGroupRequest {
            name: "ops".to_string(),
            description: None,
        });
        assert_eq!(payload.name, "ops");
        assert!(payload.description.is_none());
    }

    #[test]
    fn permission_bindings_maps_permissions_and_preserves_order() {
        let bindings = permission_bindings(vec![
            PermissionBindingRequest {
                group_id: 1,
                permission: PermissionValue::Write,
            },
            PermissionBindingRequest {
                group_id: 2,
                permission: PermissionValue::Read,
            },
        ]);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].group_id, 1);
        assert_eq!(bindings[0].permission, PermissionEnum::Write);
        assert_eq!(bindings[1].group_id, 2);
        assert_eq!(bindings[1].permission, PermissionEnum::Read);
    }
}
