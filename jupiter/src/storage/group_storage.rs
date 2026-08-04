use std::{
    collections::{BTreeMap, HashSet},
    ops::Deref,
};

use api_model::common::Pagination;
use callisto::{
    mega_group, mega_group_member, mega_resource_permission, sea_orm_active_enums::ResourceTypeEnum,
};
use common::{errors::MegaError, utils::generate_id};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait, sea_query::OnConflict,
};

use crate::{
    model::group_dto::{
        CreateGroupPayload, DeleteGroupStats, ResourcePermissionBinding, UpdateGroupPayload,
    },
    storage::base_storage::{BaseStorage, StorageConnector},
};

#[derive(Clone, Debug)]
pub struct GroupStorage {
    pub base: BaseStorage,
}

impl Deref for GroupStorage {
    type Target = BaseStorage;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl GroupStorage {
    pub async fn create_group(
        &self,
        payload: CreateGroupPayload,
    ) -> Result<mega_group::Model, MegaError> {
        let CreateGroupPayload { name, description } = payload;
        let group_id = generate_id();
        let now = chrono::Utc::now().naive_utc();
        let group = mega_group::ActiveModel {
            id: Set(group_id),
            name: Set(name.clone()),
            description: Set(description),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let on_conflict = OnConflict::column(mega_group::Column::Name)
            .do_nothing()
            .to_owned();

        mega_group::Entity::insert(group)
            .on_conflict(on_conflict)
            .exec(self.get_connection())
            .await
            .map_err(|e| match e {
                DbErr::RecordNotInserted => {
                    MegaError::Conflict(format!("Group already exists: {name}"))
                }
                _ => e.into(),
            })?;

        self.get_group_by_id(group_id)
            .await?
            .ok_or_else(|| MegaError::Other("[code:500] Failed to load created group".to_string()))
    }

    pub async fn list_groups(
        &self,
        page: Pagination,
    ) -> Result<(Vec<mega_group::Model>, u64), MegaError> {
        let paginator = mega_group::Entity::find()
            .order_by_desc(mega_group::Column::CreatedAt)
            .paginate(self.get_connection(), page.per_page);
        let total = paginator.num_items().await?;
        let items = paginator.fetch_page(page.page.saturating_sub(1)).await?;

        Ok((items, total))
    }

    pub async fn get_group_by_id(
        &self,
        group_id: i64,
    ) -> Result<Option<mega_group::Model>, MegaError> {
        Ok(mega_group::Entity::find_by_id(group_id)
            .one(self.get_connection())
            .await?)
    }

    pub async fn update_group(
        &self,
        group_id: i64,
        payload: UpdateGroupPayload,
    ) -> Result<mega_group::Model, MegaError> {
        let UpdateGroupPayload { name, description } = payload;
        let group = self
            .get_group_by_id(group_id)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("Group not found: {group_id}")))?;

        let mut active_model: mega_group::ActiveModel = group.into();
        active_model.name = Set(name.clone());
        active_model.description = Set(description);
        active_model.updated_at = Set(chrono::Utc::now().naive_utc());

        active_model
            .update(self.get_connection())
            .await
            .map_err(|e| map_unique_to_conflict(e, name))
    }

    pub async fn delete_group_with_relations(
        &self,
        group_id: i64,
    ) -> Result<DeleteGroupStats, MegaError> {
        let txn = self.get_connection().begin().await?;

        let deleted_members_count = mega_group_member::Entity::delete_many()
            .filter(mega_group_member::Column::GroupId.eq(group_id))
            .exec(&txn)
            .await?
            .rows_affected;

        let deleted_permissions_count = mega_resource_permission::Entity::delete_many()
            .filter(mega_resource_permission::Column::GroupId.eq(group_id))
            .exec(&txn)
            .await?
            .rows_affected;

        let deleted_groups_count = mega_group::Entity::delete_many()
            .filter(mega_group::Column::Id.eq(group_id))
            .exec(&txn)
            .await?
            .rows_affected;

        txn.commit().await?;

        Ok(DeleteGroupStats {
            deleted_members_count,
            deleted_permissions_count,
            deleted_groups_count,
        })
    }

    pub async fn add_group_members(
        &self,
        group_id: i64,
        usernames: &[String],
    ) -> Result<Vec<mega_group_member::Model>, MegaError> {
        let usernames = normalize_usernames(usernames);
        if usernames.is_empty() {
            return Ok(Vec::new());
        }

        let now = chrono::Utc::now().naive_utc();
        let models = usernames
            .iter()
            .map(|username| mega_group_member::ActiveModel {
                id: Set(generate_id()),
                group_id: Set(group_id),
                campsite_user_id: Set(username.clone()),
                joined_at: Set(now),
            })
            .collect::<Vec<_>>();

        let on_conflict = OnConflict::columns([
            mega_group_member::Column::GroupId,
            mega_group_member::Column::CampsiteUserId,
        ])
        .do_nothing()
        .to_owned();

        match mega_group_member::Entity::insert_many(models)
            .on_conflict(on_conflict)
            .exec(self.get_connection())
            .await
        {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(e) => {
                return Err(map_fk_to_not_found(
                    e,
                    format!("Group not found: {group_id}"),
                ));
            }
        }

        Ok(mega_group_member::Entity::find()
            .filter(mega_group_member::Column::GroupId.eq(group_id))
            .filter(mega_group_member::Column::CampsiteUserId.is_in(usernames))
            .order_by_asc(mega_group_member::Column::JoinedAt)
            .all(self.get_connection())
            .await?)
    }

    pub async fn remove_group_member(
        &self,
        group_id: i64,
        username: &str,
    ) -> Result<bool, MegaError> {
        let result = mega_group_member::Entity::delete_many()
            .filter(mega_group_member::Column::GroupId.eq(group_id))
            .filter(mega_group_member::Column::CampsiteUserId.eq(username))
            .exec(self.get_connection())
            .await?;

        Ok(result.rows_affected > 0)
    }

    pub async fn list_group_members(
        &self,
        group_id: i64,
        page: Pagination,
    ) -> Result<(Vec<mega_group_member::Model>, u64), MegaError> {
        let paginator = mega_group_member::Entity::find()
            .filter(mega_group_member::Column::GroupId.eq(group_id))
            .order_by_asc(mega_group_member::Column::JoinedAt)
            .paginate(self.get_connection(), page.per_page);
        let total = paginator.num_items().await?;
        let items = paginator.fetch_page(page.page.saturating_sub(1)).await?;

        Ok((items, total))
    }

    pub async fn find_group_ids_by_username(&self, username: &str) -> Result<Vec<i64>, MegaError> {
        Ok(mega_group_member::Entity::find()
            .select_only()
            .column(mega_group_member::Column::GroupId)
            .filter(mega_group_member::Column::CampsiteUserId.eq(username))
            .into_tuple::<i64>()
            .all(self.get_connection())
            .await?)
    }

    pub async fn find_groups_by_username(
        &self,
        username: &str,
    ) -> Result<Vec<mega_group::Model>, MegaError> {
        let group_ids = self.find_group_ids_by_username(username).await?;
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }

        Ok(mega_group::Entity::find()
            .filter(mega_group::Column::Id.is_in(group_ids))
            .order_by_asc(mega_group::Column::Name)
            .all(self.get_connection())
            .await?)
    }

    pub async fn list_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        Ok(mega_resource_permission::Entity::find()
            .filter(mega_resource_permission::Column::ResourceType.eq(resource_type))
            .filter(mega_resource_permission::Column::ResourceId.eq(resource_id))
            .order_by_asc(mega_resource_permission::Column::GroupId)
            .all(self.get_connection())
            .await?)
    }

    pub async fn replace_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        permissions: &[ResourcePermissionBinding],
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        let permissions = normalize_permission_bindings(permissions);
        let txn = self.get_connection().begin().await?;

        mega_resource_permission::Entity::delete_many()
            .filter(mega_resource_permission::Column::ResourceType.eq(resource_type.clone()))
            .filter(mega_resource_permission::Column::ResourceId.eq(resource_id))
            .exec(&txn)
            .await?;

        if !permissions.is_empty() {
            let now = chrono::Utc::now().naive_utc();
            let models = permissions
                .iter()
                .map(|binding| mega_resource_permission::ActiveModel {
                    id: Set(generate_id()),
                    resource_type: Set(resource_type.clone()),
                    resource_id: Set(resource_id.to_string()),
                    group_id: Set(binding.group_id),
                    permission: Set(binding.permission.clone()),
                    created_at: Set(now),
                    updated_at: Set(now),
                })
                .collect::<Vec<_>>();

            mega_resource_permission::Entity::insert_many(models)
                .exec(&txn)
                .await
                .map_err(|e| {
                    map_fk_to_not_found(e, "Group not found in permission binding".to_string())
                })?;
        }

        let result = mega_resource_permission::Entity::find()
            .filter(mega_resource_permission::Column::ResourceType.eq(resource_type.clone()))
            .filter(mega_resource_permission::Column::ResourceId.eq(resource_id))
            .order_by_asc(mega_resource_permission::Column::GroupId)
            .all(&txn)
            .await?;

        txn.commit().await?;
        Ok(result)
    }

    pub async fn upsert_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        permissions: &[ResourcePermissionBinding],
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        let permissions = normalize_permission_bindings(permissions);
        if permissions.is_empty() {
            return self
                .list_resource_permissions(resource_type, resource_id)
                .await;
        }

        let now = chrono::Utc::now().naive_utc();
        let models = permissions
            .iter()
            .map(|binding| mega_resource_permission::ActiveModel {
                id: Set(generate_id()),
                resource_type: Set(resource_type.clone()),
                resource_id: Set(resource_id.to_string()),
                group_id: Set(binding.group_id),
                permission: Set(binding.permission.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect::<Vec<_>>();

        let on_conflict = OnConflict::columns([
            mega_resource_permission::Column::ResourceType,
            mega_resource_permission::Column::ResourceId,
            mega_resource_permission::Column::GroupId,
        ])
        .update_columns([
            mega_resource_permission::Column::Permission,
            mega_resource_permission::Column::UpdatedAt,
        ])
        .to_owned();

        match mega_resource_permission::Entity::insert_many(models)
            .on_conflict(on_conflict)
            .exec(self.get_connection())
            .await
        {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(e) => {
                return Err(map_fk_to_not_found(
                    e,
                    "Group not found in permission binding".to_string(),
                ));
            }
        }

        self.list_resource_permissions(resource_type, resource_id)
            .await
    }

    pub async fn delete_resource_permissions(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
    ) -> Result<u64, MegaError> {
        let result = mega_resource_permission::Entity::delete_many()
            .filter(mega_resource_permission::Column::ResourceType.eq(resource_type))
            .filter(mega_resource_permission::Column::ResourceId.eq(resource_id))
            .exec(self.get_connection())
            .await?;

        Ok(result.rows_affected)
    }

    pub async fn find_permissions_by_resource(
        &self,
        resource_type: ResourceTypeEnum,
        resource_id: &str,
        group_ids: &[i64],
    ) -> Result<Vec<mega_resource_permission::Model>, MegaError> {
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }

        Ok(mega_resource_permission::Entity::find()
            .filter(mega_resource_permission::Column::ResourceType.eq(resource_type))
            .filter(mega_resource_permission::Column::ResourceId.eq(resource_id))
            .filter(mega_resource_permission::Column::GroupId.is_in(group_ids.to_vec()))
            .order_by_asc(mega_resource_permission::Column::GroupId)
            .all(self.get_connection())
            .await?)
    }
}

fn normalize_usernames(usernames: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();

    usernames
        .iter()
        .map(|username| username.trim())
        .filter(|username| !username.is_empty())
        .filter_map(|username| {
            let s = username.to_string();
            if seen.insert(s.clone()) {
                Some(s)
            } else {
                None
            }
        })
        .collect()
}

fn normalize_permission_bindings(
    permissions: &[ResourcePermissionBinding],
) -> Vec<ResourcePermissionBinding> {
    let mut by_group = BTreeMap::new();
    for permission in permissions {
        by_group.insert(permission.group_id, permission.permission.clone());
    }

    by_group
        .into_iter()
        .map(|(group_id, permission)| ResourcePermissionBinding {
            group_id,
            permission,
        })
        .collect()
}

/// Returns true when the database error indicates a foreign-key violation.
fn is_fk_constraint_error(err: &DbErr) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("foreign key constraint")
        || msg.contains("foreign key constraint failed")
        || msg.contains("violates foreign key")
        || msg.contains("cannot add or update a child row")
}

fn map_fk_to_not_found(err: DbErr, msg: String) -> MegaError {
    if is_fk_constraint_error(&err) {
        MegaError::NotFound(msg)
    } else {
        err.into()
    }
}

/// Returns true when the database error indicates a unique-key conflict.
fn is_unique_constraint_error(err: &DbErr) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("unique constraint")
        || msg.contains("unique violation")
        || msg.contains("duplicate key")
        || msg.contains("is not unique")
}

fn map_unique_to_conflict(err: DbErr, group_name: String) -> MegaError {
    if is_unique_constraint_error(&err) {
        MegaError::Conflict(format!("Group already exists: {group_name}"))
    } else {
        err.into()
    }
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::PermissionEnum;
    use common::errors::MegaError;

    use super::*;

    #[tokio::test]
    async fn create_group_conflicts_on_duplicate_name_concurrently() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;

        let group_storage = storage.group_storage();
        let payload = CreateGroupPayload {
            name: "concurrent-dup-group".to_string(),
            description: Some("test".to_string()),
        };

        let left = group_storage.clone();
        let right = group_storage.clone();
        let (res_a, res_b) = tokio::join!(
            left.create_group(payload.clone()),
            right.create_group(payload),
        );

        let mut success_count = 0;
        let mut conflict_count = 0;

        for result in [res_a, res_b] {
            match result {
                Ok(_) => success_count += 1,
                Err(MegaError::Conflict(_)) => conflict_count += 1,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        assert_eq!(success_count, 1);
        assert_eq!(conflict_count, 1);
    }

    #[tokio::test]
    async fn add_group_members_returns_not_found_when_group_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;
        let group_storage = storage.group_storage();

        let result = group_storage
            .add_group_members(999_999, &["alice".to_string()])
            .await;

        match result {
            Err(MegaError::NotFound(msg)) => assert!(msg.contains("Group not found")),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_group_returns_not_found_when_group_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;
        let group_storage = storage.group_storage();

        let result = group_storage
            .update_group(
                999_999,
                UpdateGroupPayload {
                    name: "missing-group".to_string(),
                    description: Some("missing".to_string()),
                },
            )
            .await;

        match result {
            Err(MegaError::NotFound(msg)) => assert!(msg.contains("Group not found")),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_group_conflicts_on_duplicate_name() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;
        let group_storage = storage.group_storage();

        let left = group_storage
            .create_group(CreateGroupPayload {
                name: "group-left".to_string(),
                description: None,
            })
            .await
            .expect("left group should be created");
        let right = group_storage
            .create_group(CreateGroupPayload {
                name: "group-right".to_string(),
                description: None,
            })
            .await
            .expect("right group should be created");

        let result = group_storage
            .update_group(
                right.id,
                UpdateGroupPayload {
                    name: left.name,
                    description: Some("dup".to_string()),
                },
            )
            .await;

        match result {
            Err(MegaError::Conflict(_)) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn upsert_resource_permissions_returns_not_found_when_group_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;
        let group_storage = storage.group_storage();

        let result = group_storage
            .upsert_resource_permissions(
                ResourceTypeEnum::Note,
                "1001",
                &[ResourcePermissionBinding {
                    group_id: 999_999,
                    permission: PermissionEnum::Read,
                }],
            )
            .await;

        match result {
            Err(MegaError::NotFound(msg)) => assert!(msg.contains("Group not found")),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_resource_permissions_returns_not_found_when_group_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage = crate::tests::test_storage(temp_dir.path()).await;
        let group_storage = storage.group_storage();

        let result = group_storage
            .replace_resource_permissions(
                ResourceTypeEnum::Note,
                "1002",
                &[ResourcePermissionBinding {
                    group_id: 999_999,
                    permission: PermissionEnum::Write,
                }],
            )
            .await;

        match result {
            Err(MegaError::NotFound(msg)) => assert!(msg.contains("Group not found")),
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
