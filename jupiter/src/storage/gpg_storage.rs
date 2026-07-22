use std::{ops::Deref, time::SystemTime};

use callisto::{entity_ext::generate_id, gpg_key};
use chrono::{Duration, Utc};
use common::errors::MegaError;
use pgp::{
    composed::{Deserializable, SignedPublicKey},
    types::KeyDetails,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ExprTrait, IntoActiveModel, QueryFilter,
    prelude::Expr,
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

#[derive(Clone)]
pub struct GpgStorage {
    pub base: BaseStorage,
}

impl Deref for GpgStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl GpgStorage {
    fn timestamp_to_naive_utc(ts: pgp::types::Timestamp) -> chrono::NaiveDateTime {
        chrono::DateTime::<Utc>::from(SystemTime::from(ts)).naive_utc()
    }

    fn create_key(
        &self,
        user_id: String,
        gpg_content: String,
    ) -> Result<gpg_key::Model, MegaError> {
        let (pk, _headers) = SignedPublicKey::from_string(&gpg_content).map_err(|e| {
            tracing::error!("{:?}", e);
            MegaError::Other("Failed to parse GPG key, please check format".to_string())
        })?;

        let key_id = format!("{}", pk.legacy_key_id()).to_uppercase();
        let fingerprint = format!("{:?}", pk.fingerprint());
        let created_at = Self::timestamp_to_naive_utc(pk.created_at());
        let expires_at = pk
            .legacy_v3_expiration_days()
            .map(|days| created_at + Duration::days(i64::from(days)));

        let key = gpg_key::Model {
            id: generate_id(),
            user_id,
            key_id,
            public_key: gpg_content,
            fingerprint,
            alias: "user-key".to_string(),
            created_at,
            expires_at,
        };

        Ok(key)
    }

    pub async fn add_gpg_key(&self, user_id: String, gpg_content: String) -> Result<(), MegaError> {
        let key = self.create_key(user_id, gpg_content)?;
        let a_model = key.into_active_model();
        a_model.insert(self.get_connection()).await.map_err(|e| {
            tracing::error!("{:?}", e);
            MegaError::Other("Failed to save GPG key".to_string())
        })?;
        Ok(())
    }

    pub async fn remove_gpg_key(&self, user_id: String, key_id: String) -> Result<(), MegaError> {
        gpg_key::Entity::delete_many()
            .filter(gpg_key::Column::UserId.eq(user_id))
            .filter(gpg_key::Column::KeyId.eq(key_id))
            .exec(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{:?}", e);
                MegaError::Other("Failed to delete GPG key".to_string())
            })?;
        Ok(())
    }

    pub async fn list_user_gpg(&self, user_id: String) -> Result<Vec<gpg_key::Model>, MegaError> {
        let now = Utc::now().naive_utc();

        let res: Vec<gpg_key::Model> = gpg_key::Entity::find()
            .filter(gpg_key::Column::UserId.eq(user_id))
            .filter(
                Expr::col(gpg_key::Column::ExpiresAt)
                    .is_null()
                    .or(Expr::col(gpg_key::Column::ExpiresAt).gt(now)),
            )
            .all(self.get_connection())
            .await
            .map_err(|e| {
                tracing::error!("{:?}", e);
                MegaError::Other("Failed to get GPG keys".to_string())
            })?;
        Ok(res)
    }
}

#[test]
fn test_create_key() {
    let key = "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\n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\nF5MtAwnDBeT2Qg==\n=Q/C5\n-----END PGP PUBLIC KEY BLOCK-----\n";
    let (pk, _) = SignedPublicKey::from_string(key).expect("Failed to parse");

    let key_id = format!("{}", pk.legacy_key_id()).to_uppercase();
    let fingerprint = format!("{:?}", pk.fingerprint()).to_uppercase();
    let created_at = GpgStorage::timestamp_to_naive_utc(pk.created_at());
    let expires_at = pk
        .legacy_v3_expiration_days()
        .map(|days| created_at + Duration::days(i64::from(days)));

    print!("{}", fingerprint);
    print!("{}", created_at);

    assert_eq!(key_id, "42FF407836735DBD");
    assert_eq!(fingerprint, "C59ED2DBB2531850A4F3E82942FF407836735DBD");
    assert!(expires_at.is_none());
}
