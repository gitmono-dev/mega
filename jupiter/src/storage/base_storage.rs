use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use common::errors::{MegaError, db_err_is_retryable_serialization};
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    sea_query::OnConflict,
};

const INSERT_RETRY_ATTEMPTS: u32 = 5;
const INSERT_RETRY_BASE_MS: u64 = 10;

async fn insert_many_with_deadlock_retry<E, A>(
    conn: &DatabaseConnection,
    txn: Option<&DatabaseTransaction>,
    models: Vec<A>,
    onconflict: &OnConflict,
) -> Result<(), MegaError>
where
    E: EntityTrait,
    A: ActiveModelTrait<Entity = E> + From<<E as EntityTrait>::Model> + Send + Clone,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let insert = E::insert_many(models.clone()).on_conflict(onconflict.clone());
        let result = if let Some(txn) = txn {
            insert.exec(txn).await
        } else {
            insert.exec(conn).await
        };
        match result {
            Ok(_) | Err(DbErr::RecordNotInserted) => return Ok(()),
            Err(e) if db_err_is_retryable_serialization(&e) && attempt < INSERT_RETRY_ATTEMPTS => {
                let backoff_ms = INSERT_RETRY_BASE_MS << (attempt - 1);
                tracing::warn!(
                    attempt,
                    backoff_ms,
                    error = %e,
                    "retrying batch insert after deadlock or serialization failure"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[async_trait]
pub trait StorageConnector {
    const BATCH_CHUNK_SIZE: usize = 1000;

    fn get_connection(&self) -> &DatabaseConnection;

    fn mock() -> Self;

    fn new(connection: Arc<DatabaseConnection>) -> Self;

    /// Performs batch saving of models in the database.
    async fn batch_save_model<E, A>(&self, save_models: Vec<A>) -> Result<(), MegaError>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E> + From<<E as EntityTrait>::Model> + Send + Clone,
    {
        let onconflict = OnConflict::new().do_nothing().to_owned();
        Self::batch_save_model_with_conflict(self, save_models, onconflict).await
    }

    async fn batch_save_model_with_txn<E, A>(
        &self,
        save_models: Vec<A>,
        txn: Option<&DatabaseTransaction>,
    ) -> Result<(), MegaError>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E> + From<<E as EntityTrait>::Model> + Send + Clone,
    {
        let onconflict = OnConflict::new().do_nothing().to_owned();
        Self::batch_save_model_with_conflict_and_txn(self, save_models, onconflict, txn).await
    }

    async fn batch_save_model_with_conflict_and_txn<E, A>(
        &self,
        save_models: Vec<A>,
        onconflict: OnConflict,
        txn: Option<&DatabaseTransaction>,
    ) -> Result<(), MegaError>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E> + From<<E as EntityTrait>::Model> + Send + Clone,
    {
        let mut i = 0;
        let len = save_models.len();

        while i < len {
            let end = (i + Self::BATCH_CHUNK_SIZE).min(len);
            insert_many_with_deadlock_retry::<E, A>(
                self.get_connection(),
                txn,
                save_models[i..end].to_vec(),
                &onconflict,
            )
            .await?;
            i = end;
        }
        Ok(())
    }

    /// Performs batch saving of models in the database with conflict resolution.
    ///
    /// This function allows saving models in batches while specifying conflict resolution behavior using the `OnConflict` parameter.
    /// It is intended for advanced use cases where fine-grained control over conflict handling is required.
    ///
    /// # Arguments
    ///
    /// * `save_models` - A vector of models to be saved.
    /// * `onconflict` - Specifies the conflict resolution strategy to be used during insertion.
    ///
    /// # Generic Constraints
    ///
    /// * `E` - The entity type that implements the `EntityTrait` trait.
    /// * `A` - The model type that implements the `ActiveModelTrait` trait and is convertible from the corresponding model type of `E`.
    ///
    /// # Errors
    ///
    /// Returns a `MegaError` if an error occurs during the batch save operation.
    /// Note: The function ignores `DbErr::RecordNotInserted` errors, which may lead to silent failures.
    /// Use this function with caution and ensure that the `OnConflict` parameter is configured correctly to avoid unintended consequences.
    async fn batch_save_model_with_conflict<E, A>(
        &self,
        save_models: Vec<A>,
        onconflict: OnConflict,
    ) -> Result<(), MegaError>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E> + From<<E as EntityTrait>::Model> + Send + Clone,
    {
        let mut i = 0;
        let len = save_models.len();

        while i < len {
            let end = (i + Self::BATCH_CHUNK_SIZE).min(len);
            insert_many_with_deadlock_retry::<E, A>(
                self.get_connection(),
                None,
                save_models[i..end].to_vec(),
                &onconflict,
            )
            .await?;
            i = end;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BaseStorage {
    pub connection: Arc<DatabaseConnection>,
}

impl StorageConnector for BaseStorage {
    fn get_connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    fn mock() -> Self {
        Self {
            connection: Arc::new(DatabaseConnection::default()),
        }
    }

    fn new(connection: Arc<DatabaseConnection>) -> Self {
        Self { connection }
    }
}
