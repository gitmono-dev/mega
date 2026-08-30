use std::ops::Deref;

use callisto::{lfs_locks, lfs_objects};
use common::errors::MegaError;
use sea_orm::{EntityTrait, InsertResult, IntoActiveModel, Set, TryInsertResult};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone)]
pub struct LfsDbStorage {
    pub base: BaseStorage,
}

impl Deref for LfsDbStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl LfsDbStorage {
    pub async fn new_lfs_object(&self, object: lfs_objects::Model) -> Result<bool, MegaError> {
        let res = lfs_objects::Entity::insert(object.into_active_model())
            .on_conflict_do_nothing()
            .exec(self.get_connection())
            .await?;
        Ok(matches!(res, TryInsertResult::Inserted(_)))
    }

    pub async fn get_lfs_object(&self, oid: &str) -> Result<Option<lfs_objects::Model>, MegaError> {
        let result = lfs_objects::Entity::find_by_id(oid)
            .one(self.get_connection())
            .await?;
        Ok(result)
    }

    pub async fn delete_lfs_object(&self, oid: String) -> Result<(), MegaError> {
        lfs_objects::Entity::delete_by_id(oid)
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn new_lock(
        &self,
        lfs_lock: lfs_locks::Model,
    ) -> Result<InsertResult<lfs_locks::ActiveModel>, MegaError> {
        Ok(lfs_locks::Entity::insert(lfs_lock.into_active_model())
            .exec(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn get_lock_by_id(
        &self,
        refspec: &str,
    ) -> Result<Option<lfs_locks::Model>, MegaError> {
        let result = lfs_locks::Entity::find_by_id(refspec)
            .one(self.get_connection())
            .await
            .unwrap();
        Ok(result)
    }

    pub async fn update_lock(
        &self,
        lfs_lock: lfs_locks::Model,
        data: &str,
    ) -> Result<lfs_locks::Model, MegaError> {
        let mut val = lfs_lock.into_active_model();
        val.data = Set(data.to_owned());
        Ok(lfs_locks::Entity::update(val)
            .exec(self.get_connection())
            .await
            .unwrap())
    }

    pub async fn delete_lock_by_id(&self, id: String) {
        lfs_locks::Entity::delete_by_id(id)
            .exec(self.get_connection())
            .await
            .unwrap();
    }
}
