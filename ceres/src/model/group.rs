use callisto::{
    mega_group, mega_group_member, mega_resource_permission,
    sea_orm_active_enums::{PermissionEnum, ResourceTypeEnum},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmptyListAdditional {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateGroupRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddMembersRequest {
    /// Campsite public user ids (field name kept for API compat).
    pub usernames: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupMemberResponse {
    pub id: i64,
    pub group_id: i64,
    /// Campsite public user id (field name kept for API compat).
    pub username: String,
    pub joined_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionValue {
    Read,
    Write,
    Admin,
}

impl PermissionValue {
    pub fn level(self) -> u8 {
        match self {
            PermissionValue::Read => 1,
            PermissionValue::Write => 2,
            PermissionValue::Admin => 3,
        }
    }

    pub fn satisfies(self, required: PermissionValue) -> bool {
        self.level() >= required.level()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceTypeValue {
    Note,
}

impl TryFrom<&str> for ResourceTypeValue {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "note" => Ok(ResourceTypeValue::Note),
            _ => Err(format!("Invalid resource_type: {}", value)),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PermissionBindingRequest {
    pub group_id: i64,
    pub permission: PermissionValue,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetPermissionsRequest {
    pub permissions: Vec<PermissionBindingRequest>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResourcePermissionResponse {
    pub id: i64,
    pub resource_type: ResourceTypeValue,
    pub resource_id: String,
    pub group_id: i64,
    pub permission: PermissionValue,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteGroupResponse {
    pub group_id: i64,
    pub deleted_members_count: u64,
    pub deleted_permissions_count: u64,
    pub deleted_groups_count: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RemoveMemberResponse {
    pub group_id: i64,
    pub username: String,
    pub removed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeletePermissionsResponse {
    pub resource_type: ResourceTypeValue,
    pub resource_id: String,
    pub deleted_count: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserGroupsResponse {
    pub username: String,
    pub groups: Vec<GroupResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserEffectivePermissionResponse {
    pub username: String,
    pub resource_type: ResourceTypeValue,
    pub resource_id: String,
    pub is_admin: bool,
    pub permission: Option<PermissionValue>,
    pub has_read: bool,
    pub has_write: bool,
    pub has_admin: bool,
}

impl From<mega_group::Model> for GroupResponse {
    fn from(value: mega_group::Model) -> Self {
        Self {
            id: value.id,
            name: value.name,
            description: value.description,
            created_at: value.created_at.and_utc().timestamp(),
            updated_at: value.updated_at.and_utc().timestamp(),
        }
    }
}

impl From<mega_group_member::Model> for GroupMemberResponse {
    fn from(value: mega_group_member::Model) -> Self {
        Self {
            id: value.id,
            group_id: value.group_id,
            username: value.campsite_user_id,
            joined_at: value.joined_at.and_utc().timestamp(),
        }
    }
}

impl From<mega_resource_permission::Model> for ResourcePermissionResponse {
    fn from(value: mega_resource_permission::Model) -> Self {
        Self {
            id: value.id,
            resource_type: value.resource_type.into(),
            resource_id: value.resource_id,
            group_id: value.group_id,
            permission: value.permission.into(),
            created_at: value.created_at.and_utc().timestamp(),
            updated_at: value.updated_at.and_utc().timestamp(),
        }
    }
}

impl From<PermissionValue> for PermissionEnum {
    fn from(value: PermissionValue) -> Self {
        match value {
            PermissionValue::Read => PermissionEnum::Read,
            PermissionValue::Write => PermissionEnum::Write,
            PermissionValue::Admin => PermissionEnum::Admin,
        }
    }
}

impl From<PermissionEnum> for PermissionValue {
    fn from(value: PermissionEnum) -> Self {
        match value {
            PermissionEnum::Read => PermissionValue::Read,
            PermissionEnum::Write => PermissionValue::Write,
            PermissionEnum::Admin => PermissionValue::Admin,
        }
    }
}

impl From<ResourceTypeValue> for ResourceTypeEnum {
    fn from(value: ResourceTypeValue) -> Self {
        match value {
            ResourceTypeValue::Note => ResourceTypeEnum::Note,
        }
    }
}

impl From<ResourceTypeEnum> for ResourceTypeValue {
    fn from(value: ResourceTypeEnum) -> Self {
        match value {
            ResourceTypeEnum::Note => ResourceTypeValue::Note,
        }
    }
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::PermissionEnum;

    use super::{PermissionValue, ResourceTypeValue};

    #[test]
    fn permission_value_satisfies_hierarchy() {
        assert!(PermissionValue::Admin.satisfies(PermissionValue::Write));
        assert!(PermissionValue::Write.satisfies(PermissionValue::Read));
        assert!(!PermissionValue::Read.satisfies(PermissionValue::Write));
    }

    #[test]
    fn resource_type_value_try_from_note() {
        assert_eq!(
            ResourceTypeValue::try_from("note").unwrap(),
            ResourceTypeValue::Note
        );
        assert!(ResourceTypeValue::try_from("issue").is_err());
    }

    #[test]
    fn permission_value_round_trips_with_permission_enum() {
        let write = PermissionValue::Write;
        let as_enum: PermissionEnum = write.into();
        let back: PermissionValue = as_enum.into();
        assert_eq!(back, write);
    }
}
