use std::ops::Deref;

use callisto::orion_vm_image;
use chrono::Utc;
use common::errors::MegaError;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, Set,
};
use uuid::Uuid;

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone)]
pub struct OrionVmImageStorage {
    pub base: BaseStorage,
}

impl Deref for OrionVmImageStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpsertOrionVmImage {
    pub digest: String,
    pub object_key: String,
    pub info_object_key: Option<String>,
    pub image_name: Option<String>,
    pub built_at: Option<String>,
    pub rust: Option<String>,
    pub buck2: Option<String>,
    pub python: Option<String>,
    pub kernel: Option<String>,
    pub size_bytes: Option<i64>,
    pub label: Option<String>,
}

impl OrionVmImageStorage {
    pub async fn list_all(&self) -> Result<Vec<orion_vm_image::Model>, MegaError> {
        Ok(orion_vm_image::Entity::find()
            .order_by_desc(orion_vm_image::Column::CreatedAt)
            .all(self.get_connection())
            .await?)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<orion_vm_image::Model>, MegaError> {
        Ok(orion_vm_image::Entity::find_by_id(id.to_string())
            .one(self.get_connection())
            .await?)
    }

    pub async fn find_by_digest(
        &self,
        digest: &str,
    ) -> Result<Option<orion_vm_image::Model>, MegaError> {
        Ok(orion_vm_image::Entity::find()
            .filter(orion_vm_image::Column::Digest.eq(digest))
            .one(self.get_connection())
            .await?)
    }

    pub async fn upsert(
        &self,
        input: UpsertOrionVmImage,
    ) -> Result<orion_vm_image::Model, MegaError> {
        if let Some(existing) = self.find_by_digest(&input.digest).await? {
            let mut am = existing.into_active_model();
            am.object_key = Set(input.object_key);
            am.info_object_key = Set(input.info_object_key);
            am.image_name = Set(input.image_name);
            am.built_at = Set(input.built_at);
            am.rust = Set(input.rust);
            am.buck2 = Set(input.buck2);
            am.python = Set(input.python);
            am.kernel = Set(input.kernel);
            am.size_bytes = Set(input.size_bytes);
            am.label = Set(input.label);
            Ok(am.update(self.get_connection()).await?)
        } else {
            let model = orion_vm_image::Model {
                id: Uuid::new_v4().to_string(),
                digest: input.digest,
                object_key: input.object_key,
                info_object_key: input.info_object_key,
                image_name: input.image_name,
                built_at: input.built_at,
                rust: input.rust,
                buck2: input.buck2,
                python: input.python,
                kernel: input.kernel,
                size_bytes: input.size_bytes,
                label: input.label,
                created_at: Utc::now().naive_utc(),
            };
            Ok(model
                .into_active_model()
                .insert(self.get_connection())
                .await?)
        }
    }

    pub async fn delete_by_id(&self, id: &str) -> Result<Option<orion_vm_image::Model>, MegaError> {
        let Some(model) = self.find_by_id(id).await? else {
            return Ok(None);
        };
        orion_vm_image::Entity::delete_by_id(id.to_string())
            .exec(self.get_connection())
            .await?;
        Ok(Some(model))
    }
}
