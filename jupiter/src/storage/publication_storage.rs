//! Transaction owner for namespace metadata. Reserve an operation BEFORE any
//! ref write; either return its committed receipt or lend the same transaction
//! to the writer. This core does not authorize writers, verify Git objects,
//! apply binding policy, or supply retention pins. Ceres must provide those
//! gates before a production capability can be enabled.

use callisto::{
    namespace_head, namespace_outbox, namespace_publication, namespace_view, snapshot_instance,
    snapshot_operation,
};
use common::errors::MegaError;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseTransaction, DbErr, EntityTrait, QueryFilter, Set,
    TransactionTrait,
    sea_query::{Expr, OnConflict},
};

use super::{
    base_storage::{BaseStorage, StorageConnector},
    namespace_storage::{MAX_NAMESPACE_NODE_BYTES, node_digest, validate_digest},
};

#[derive(Clone)]
pub struct PublicationStorage {
    pub base: BaseStorage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationHead {
    pub publication_seq: i64,
    pub view_id: String,
    pub writer_epoch: i64,
}

/// The authenticated caller supplies actor_domain; it is not a client-selected
/// authorization grant. request_digest must cover the complete canonical plan,
/// including expected refs/head, binding policy and prepared content identities.
#[derive(Debug, Clone)]
pub struct PublicationRequest {
    pub instance_id: String,
    pub actor_domain: String,
    pub operation_id: String,
    pub request_digest: String,
}

/// Ceres supplies verified namespace-manifest-v1 bytes. Jupiter checks the byte
/// bound, identity and immutability, not the higher-level descriptor semantics.
#[derive(Debug, Clone)]
pub struct PreparedNamespaceView {
    pub instance_id: String,
    pub view_id: String,
    pub canonical_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationOutcome {
    Published,
    NoOp,
}

impl PublicationOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::NoOp => "no_op",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationReceipt {
    pub publication_seq: i64,
    pub view_id: String,
    pub outcome: PublicationOutcome,
}

pub enum BeginPublication {
    Replay(PublicationReceipt),
    Ready(Box<PublicationTransaction>),
}

/// Never expose ownership of the underlying transaction: only finish can
/// commit the reserved operation, and abort/drop rolls back staged ref writes.
pub struct PublicationTransaction {
    storage: PublicationStorage,
    transaction: DatabaseTransaction,
    request: PublicationRequest,
    expected: Option<PublicationHead>,
    writer_epoch: i64,
}

impl PublicationStorage {
    pub async fn head(&self, instance: &str) -> Result<Option<PublicationHead>, MegaError> {
        self.head_in(self.base.get_connection(), instance).await
    }

    async fn head_in<C: ConnectionTrait>(
        &self,
        conn: &C,
        instance: &str,
    ) -> Result<Option<PublicationHead>, MegaError> {
        validate_uuid(instance)?;
        let row = namespace_head::Entity::find_by_id(instance.to_owned())
            .one(conn)
            .await?;
        row.map(|row| {
            let head = PublicationHead {
                publication_seq: row.publication_seq,
                view_id: row.view_id,
                writer_epoch: row.writer_epoch,
            };
            validate_head(&head)?;
            Ok(head)
        })
        .transpose()
    }

    pub async fn view(&self, instance: &str, id: &str) -> Result<Option<Vec<u8>>, MegaError> {
        validate_uuid(instance)?;
        validate_digest(id)?;
        let row = namespace_view::Entity::find_by_id(id.to_owned())
            .filter(namespace_view::Column::InstanceId.eq(instance))
            .one(self.base.get_connection())
            .await?;
        row.map(|row| {
            validate_view_bytes(&row.view_id, &row.canonical_bytes)?;
            Ok(row.canonical_bytes)
        })
        .transpose()
    }

    /// Receipt lookup after a lost response or uncertain COMMIT. The endpoint
    /// must independently authenticate the same actor domain on every lookup.
    pub async fn receipt(
        &self,
        request: &PublicationRequest,
    ) -> Result<Option<PublicationReceipt>, MegaError> {
        validate_request(request)?;
        let row = snapshot_operation::Entity::find_by_id((
            request.actor_domain.clone(),
            request.operation_id.clone(),
        ))
        .one(self.base.get_connection())
        .await?;
        row.map(|row| receipt_from(row, request)).transpose()
    }

    pub async fn begin(
        &self,
        request: PublicationRequest,
        expected: Option<PublicationHead>,
        writer_epoch: i64,
    ) -> Result<BeginPublication, MegaError> {
        validate_request(&request)?;
        if writer_epoch <= 0 {
            return Err(MegaError::bad_request("invalid writer epoch"));
        }
        if let Some(head) = &expected {
            validate_head(head)?;
            if head.writer_epoch != writer_epoch {
                return Err(MegaError::Conflict("writer epoch mismatch".into()));
            }
        }
        let transaction = self.base.get_connection().begin().await?;
        match self.reserve(&transaction, &request, &expected).await {
            Ok(Some(receipt)) => {
                transaction.rollback().await?;
                Ok(BeginPublication::Replay(receipt))
            }
            Ok(None) => Ok(BeginPublication::Ready(Box::new(PublicationTransaction {
                storage: self.clone(),
                transaction,
                request,
                expected,
                writer_epoch,
            }))),
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    async fn reserve(
        &self,
        txn: &DatabaseTransaction,
        request: &PublicationRequest,
        expected: &Option<PublicationHead>,
    ) -> Result<Option<PublicationReceipt>, MegaError> {
        // A unique insert serializes duplicate operations even on PostgreSQL.
        // SQLite obtains its write reservation before reading mutable head state.
        let result = snapshot_operation::Entity::insert(snapshot_operation::ActiveModel {
            actor_domain: Set(request.actor_domain.clone()),
            operation_id: Set(request.operation_id.clone()),
            instance_id: Set(request.instance_id.clone()),
            request_digest: Set(request.request_digest.clone()),
            publication_seq: Set(None),
            view_id: Set(None),
            outcome: Set(None),
            created_at: Set(chrono::Utc::now().fixed_offset()),
        })
        .on_conflict(
            OnConflict::columns([
                snapshot_operation::Column::ActorDomain,
                snapshot_operation::Column::OperationId,
            ])
            .do_nothing()
            .to_owned(),
        )
        .exec(txn)
        .await;
        match result {
            Ok(_) => {}
            Err(DbErr::RecordNotInserted) => {
                let row = snapshot_operation::Entity::find_by_id((
                    request.actor_domain.clone(),
                    request.operation_id.clone(),
                ))
                .one(txn)
                .await?
                .ok_or_else(|| MegaError::Unavailable("operation disappeared".into()))?;
                return receipt_from(row, request).map(Some);
            }
            Err(error) => return Err(error.into()),
        }
        let registered = snapshot_instance::Entity::find()
            .filter(snapshot_instance::Column::InstanceId.eq(&request.instance_id))
            .one(txn)
            .await?;
        if registered.is_none() {
            return Err(MegaError::NotFound(
                "snapshot instance not registered".into(),
            ));
        }
        if &self.head_in(txn, &request.instance_id).await? != expected {
            return Err(MegaError::Conflict(
                "expected namespace head mismatch".into(),
            ));
        }
        Ok(None)
    }

    async fn put_view(
        &self,
        txn: &DatabaseTransaction,
        view: &PreparedNamespaceView,
    ) -> Result<(), MegaError> {
        validate_view_bytes(&view.view_id, &view.canonical_bytes)?;
        let inserted = namespace_view::Entity::insert(namespace_view::ActiveModel {
            view_id: Set(view.view_id.clone()),
            instance_id: Set(view.instance_id.clone()),
            canonical_bytes: Set(view.canonical_bytes.clone()),
            created_at: Set(chrono::Utc::now().fixed_offset()),
        })
        .on_conflict(
            OnConflict::column(namespace_view::Column::ViewId)
                .do_nothing()
                .to_owned(),
        )
        .exec(txn)
        .await;
        match inserted {
            Ok(_) | Err(DbErr::RecordNotInserted) => {}
            Err(e) => return Err(e.into()),
        }
        let stored = namespace_view::Entity::find_by_id(view.view_id.clone())
            .one(txn)
            .await?
            .ok_or_else(|| MegaError::Unavailable("namespace view disappeared".into()))?;
        if stored.instance_id != view.instance_id || stored.canonical_bytes != view.canonical_bytes
        {
            return Err(MegaError::Conflict(
                "immutable namespace view mismatch".into(),
            ));
        }
        Ok(())
    }
}

impl PublicationTransaction {
    pub fn transaction(&self) -> &DatabaseTransaction {
        &self.transaction
    }

    pub async fn abort(self) -> Result<(), MegaError> {
        self.transaction.rollback().await?;
        Ok(())
    }

    pub async fn finish(
        self,
        view: &PreparedNamespaceView,
        reason: &str,
    ) -> Result<PublicationReceipt, MegaError> {
        let receipt = match self.stage_finish(view, reason).await {
            Ok(receipt) => receipt,
            Err(error) => {
                self.transaction.rollback().await?;
                return Err(error);
            }
        };
        self.transaction.commit().await.map_err(|_| {
            MegaError::Unavailable(
                "publication commit outcome uncertain; query operation receipt before retrying"
                    .into(),
            )
        })?;
        Ok(receipt)
    }

    async fn stage_finish(
        &self,
        view: &PreparedNamespaceView,
        reason: &str,
    ) -> Result<PublicationReceipt, MegaError> {
        if view.instance_id != self.request.instance_id {
            return Err(MegaError::bad_request("view instance mismatch"));
        }
        validate_label(reason)?;
        let changed = self
            .expected
            .as_ref()
            .is_none_or(|head| head.view_id != view.view_id);
        let old_seq = self
            .expected
            .as_ref()
            .map_or(0, |head| head.publication_seq);
        let seq = if changed {
            old_seq
                .checked_add(1)
                .ok_or_else(|| MegaError::Unavailable("publication sequence exhausted".into()))?
        } else {
            old_seq
        };
        self.storage.put_view(&self.transaction, view).await?;
        self.cas_head(seq, &view.view_id).await?;
        let now = chrono::Utc::now().fixed_offset();
        if changed {
            namespace_publication::Entity::insert(namespace_publication::ActiveModel {
                instance_id: Set(self.request.instance_id.clone()),
                publication_seq: Set(seq),
                view_id: Set(view.view_id.clone()),
                parent_seq: Set(self.expected.as_ref().map(|h| h.publication_seq)),
                parent_view_id: Set(self.expected.as_ref().map(|h| h.view_id.clone())),
                writer_epoch: Set(self.writer_epoch),
                actor_domain: Set(self.request.actor_domain.clone()),
                operation_id: Set(self.request.operation_id.clone()),
                reason: Set(reason.into()),
                created_at: Set(now),
            })
            .exec(&self.transaction)
            .await?;
            namespace_outbox::Entity::insert(namespace_outbox::ActiveModel {
                event_id: Set(uuid::Uuid::new_v4().to_string()),
                instance_id: Set(self.request.instance_id.clone()),
                publication_seq: Set(seq),
                view_id: Set(view.view_id.clone()),
                delivered: Set(false),
                created_at: Set(now),
            })
            .exec(&self.transaction)
            .await?;
        }
        let outcome = if changed {
            PublicationOutcome::Published
        } else {
            PublicationOutcome::NoOp
        };
        let updated = snapshot_operation::Entity::update_many()
            .col_expr(snapshot_operation::Column::PublicationSeq, Expr::value(seq))
            .col_expr(
                snapshot_operation::Column::ViewId,
                Expr::value(view.view_id.clone()),
            )
            .col_expr(
                snapshot_operation::Column::Outcome,
                Expr::value(outcome.as_str()),
            )
            .filter(snapshot_operation::Column::ActorDomain.eq(&self.request.actor_domain))
            .filter(snapshot_operation::Column::OperationId.eq(&self.request.operation_id))
            .filter(snapshot_operation::Column::RequestDigest.eq(&self.request.request_digest))
            .filter(snapshot_operation::Column::Outcome.is_null())
            .exec(&self.transaction)
            .await?;
        if updated.rows_affected != 1 {
            return Err(MegaError::Conflict("operation reservation changed".into()));
        }
        Ok(PublicationReceipt {
            publication_seq: seq,
            view_id: view.view_id.clone(),
            outcome,
        })
    }

    async fn cas_head(&self, seq: i64, view_id: &str) -> Result<(), MegaError> {
        if let Some(expected) = &self.expected {
            // Even a no-op performs the fence; stale writers cannot commit ref
            // mutations simply because their proposed view did not change.
            let updated = namespace_head::Entity::update_many()
                .col_expr(namespace_head::Column::PublicationSeq, Expr::value(seq))
                .col_expr(namespace_head::Column::ViewId, Expr::value(view_id))
                .filter(namespace_head::Column::InstanceId.eq(&self.request.instance_id))
                .filter(namespace_head::Column::PublicationSeq.eq(expected.publication_seq))
                .filter(namespace_head::Column::ViewId.eq(&expected.view_id))
                .filter(namespace_head::Column::WriterEpoch.eq(self.writer_epoch))
                .exec(&self.transaction)
                .await?;
            if updated.rows_affected != 1 {
                return Err(MegaError::Conflict(
                    "expected namespace head mismatch".into(),
                ));
            }
        } else {
            let inserted = namespace_head::Entity::insert(namespace_head::ActiveModel {
                instance_id: Set(self.request.instance_id.clone()),
                publication_seq: Set(seq),
                view_id: Set(view_id.into()),
                writer_epoch: Set(self.writer_epoch),
            })
            .on_conflict(
                OnConflict::column(namespace_head::Column::InstanceId)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&self.transaction)
            .await;
            match inserted {
                Ok(_) => {}
                Err(DbErr::RecordNotInserted) => {
                    return Err(MegaError::Conflict(
                        "namespace head already initialized".into(),
                    ));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

fn validate_request(request: &PublicationRequest) -> Result<(), MegaError> {
    validate_uuid(&request.instance_id)?;
    validate_uuid(&request.operation_id)?;
    validate_label(&request.actor_domain)?;
    validate_digest(&request.request_digest)
}
fn validate_uuid(value: &str) -> Result<(), MegaError> {
    if !uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value) {
        return Err(MegaError::bad_request("invalid publication UUID"));
    }
    Ok(())
}
fn validate_label(value: &str) -> Result<(), MegaError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err(MegaError::bad_request("invalid publication label"));
    }
    Ok(())
}
fn validate_head(head: &PublicationHead) -> Result<(), MegaError> {
    if head.publication_seq <= 0 || head.writer_epoch <= 0 {
        return Err(MegaError::bad_request("invalid publication head counters"));
    }
    validate_digest(&head.view_id)
}
fn validate_view_bytes(id: &str, bytes: &[u8]) -> Result<(), MegaError> {
    validate_digest(id)?;
    if bytes.len() > MAX_NAMESPACE_NODE_BYTES || node_digest(bytes) != id {
        return Err(MegaError::Unavailable(
            "invalid namespace view content".into(),
        ));
    }
    Ok(())
}
fn receipt_from(
    row: snapshot_operation::Model,
    request: &PublicationRequest,
) -> Result<PublicationReceipt, MegaError> {
    if row.instance_id != request.instance_id || row.request_digest != request.request_digest {
        return Err(MegaError::Conflict(
            "operation key reused with a different request".into(),
        ));
    }
    let (Some(seq), Some(view_id), Some(outcome)) = (row.publication_seq, row.view_id, row.outcome)
    else {
        return Err(MegaError::Unavailable(
            "incomplete publication receipt".into(),
        ));
    };
    if seq <= 0 {
        return Err(MegaError::Unavailable("invalid publication receipt".into()));
    }
    validate_digest(&view_id)?;
    let outcome = match outcome.as_str() {
        "published" => PublicationOutcome::Published,
        "no_op" => PublicationOutcome::NoOp,
        _ => return Err(MegaError::Unavailable("unknown publication outcome".into())),
    };
    Ok(PublicationReceipt {
        publication_seq: seq,
        view_id,
        outcome,
    })
}

#[cfg(test)]
mod tests;
