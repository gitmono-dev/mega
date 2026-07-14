pub mod artifact_storage;
pub mod audit_storage;
pub mod base_storage;
pub mod bots_storage;
pub mod buck_storage;
pub mod build_trigger_storage;
pub mod cl_reviewer_storage;
pub mod cl_storage;
pub mod cla_storage;
pub mod code_review_comment_storage;
pub mod code_review_thread_storage;
pub mod commit_binding_storage;
pub mod conversation_storage;
pub mod dynamic_sidebar_storage;
pub mod git_db_storage;
pub mod gpg_storage;
pub mod group_storage;
pub mod init;
pub mod issue_storage;
pub mod lfs_db_storage;
pub mod merge_queue_storage;
pub mod mono_storage;
pub mod notification_storage;
pub mod user_approval_storage;
pub use notification_storage::NotificationStorage;
pub mod note_storage;
pub mod stg_common;
pub mod user_storage;
pub mod vault_storage;
pub mod webhook_storage;

use std::sync::{Arc, LazyLock, Weak};

use common::{config::Config, errors::MegaError};
use io_orbit::factory::ObjectStorageFactory;
use tokio::sync::Semaphore;

use crate::{
    service::{
        artifact_service::ArtifactService, buck_service::BuckService, cl_service::CLService,
        cla_service::ClaService, code_review_service::CodeReviewService, git_service::GitService,
        import_service::ImportService, issue_service::IssueService, lfs_service::LfsService,
        merge_queue_service::MergeQueueService, mono_service::MonoService,
        webhook_service::WebhookService,
    },
    storage::{
        audit_storage::AuditStorage,
        base_storage::{BaseStorage, StorageConnector},
        bots_storage::BotsStorage,
        buck_storage::BuckStorage,
        build_trigger_storage::BuildTriggerStorage,
        cl_reviewer_storage::ClReviewerStorage,
        cl_storage::ClStorage,
        cla_storage::ClaStorage,
        code_review_comment_storage::CodeReviewCommentStorage,
        code_review_thread_storage::CodeReviewThreadStorage,
        commit_binding_storage::CommitBindingStorage,
        conversation_storage::ConversationStorage,
        dynamic_sidebar_storage::DynamicSidebarStorage,
        git_db_storage::GitDbStorage,
        gpg_storage::GpgStorage,
        group_storage::GroupStorage,
        init::database_connection,
        issue_storage::IssueStorage,
        lfs_db_storage::LfsDbStorage,
        merge_queue_storage::MergeQueueStorage,
        mono_storage::MonoStorage,
        note_storage::NoteStorage,
        user_approval_storage::UserApprovalStorage,
        user_storage::UserStorage,
        vault_storage::VaultStorage,
        webhook_storage::WebhookStorage,
    },
};

#[derive(Clone)]
pub struct AppService {
    pub mono_storage: MonoStorage,
    pub git_db_storage: GitDbStorage,
    pub gpg_storage: GpgStorage,
    pub lfs_db_storage: LfsDbStorage,
    pub cla_storage: ClaStorage,
    pub user_approval_storage: UserApprovalStorage,
    pub user_storage: UserStorage,
    pub group_storage: GroupStorage,
    pub vault_storage: VaultStorage,
    pub cl_storage: ClStorage,
    pub issue_storage: IssueStorage,
    pub conversation_storage: ConversationStorage,
    pub note_storage: NoteStorage,
    pub commit_binding_storage: CommitBindingStorage,
    pub reviewer_storage: ClReviewerStorage,
    pub merge_queue_storage: MergeQueueStorage,
    pub buck_storage: BuckStorage,
    pub dynamic_sidebar_storage: DynamicSidebarStorage,
    pub code_review_comment_storage: CodeReviewCommentStorage,
    pub code_review_thread_storage: CodeReviewThreadStorage,
    pub build_trigger_storage: BuildTriggerStorage,
    pub bots_storage: BotsStorage,
    pub webhook_storage: WebhookStorage,
    pub audit_storage: AuditStorage,
    pub notification_storage: NotificationStorage,
}

impl AppService {
    fn mock() -> Arc<Self> {
        let mock = BaseStorage::mock();
        let notification_storage = NotificationStorage::new(mock.connection.clone());
        // For tests and in-memory workflows we don't need a real persistent
        // object storage. Use a filesystem-backed storage rooted in the system
        // temp directory to provide a lightweight implementation.
        Arc::new(Self {
            mono_storage: MonoStorage { base: mock.clone() },
            git_db_storage: GitDbStorage { base: mock.clone() },
            gpg_storage: GpgStorage { base: mock.clone() },
            lfs_db_storage: LfsDbStorage { base: mock.clone() },
            cla_storage: ClaStorage { base: mock.clone() },
            user_approval_storage: UserApprovalStorage { base: mock.clone() },
            user_storage: UserStorage { base: mock.clone() },
            group_storage: GroupStorage { base: mock.clone() },
            vault_storage: VaultStorage { base: mock.clone() },
            cl_storage: ClStorage { base: mock.clone() },
            issue_storage: IssueStorage { base: mock.clone() },
            conversation_storage: ConversationStorage { base: mock.clone() },
            note_storage: NoteStorage { base: mock.clone() },
            commit_binding_storage: CommitBindingStorage { base: mock.clone() },
            reviewer_storage: ClReviewerStorage { base: mock.clone() },
            merge_queue_storage: MergeQueueStorage::new(mock.clone()),
            buck_storage: BuckStorage { base: mock.clone() },
            dynamic_sidebar_storage: DynamicSidebarStorage { base: mock.clone() },
            code_review_comment_storage: CodeReviewCommentStorage { base: mock.clone() },
            code_review_thread_storage: CodeReviewThreadStorage { base: mock.clone() },
            build_trigger_storage: BuildTriggerStorage { base: mock.clone() },
            notification_storage,
            bots_storage: BotsStorage { base: mock.clone() },
            webhook_storage: WebhookStorage { base: mock.clone() },
            audit_storage: AuditStorage { base: mock.clone() },
        })
    }
}

#[derive(Clone)]
pub struct Storage {
    pub(crate) app_service: Arc<AppService>,
    pub cla_service: ClaService,
    pub issue_service: IssueService,
    pub cl_service: CLService,
    pub merge_queue_service: MergeQueueService,
    pub artifact_service: ArtifactService,
    pub buck_service: BuckService,
    pub mono_service: MonoService,
    pub import_service: ImportService,
    pub git_service: GitService,
    pub lfs_service: LfsService,
    pub config: Weak<Config>,
    pub code_review_service: CodeReviewService,
    pub webhook_service: WebhookService,
}

impl Storage {
    pub async fn new(config: Arc<Config>) -> Result<Self, MegaError> {
        let connection = Arc::new(database_connection(&config.database).await);
        let base = BaseStorage::new(connection.clone());
        let notification_storage = NotificationStorage::new(connection.clone());

        let mono_storage = MonoStorage { base: base.clone() };
        let git_db_storage = GitDbStorage { base: base.clone() };
        let gpg_storage = GpgStorage { base: base.clone() };
        let lfs_db_storage = LfsDbStorage { base: base.clone() };
        let cla_storage = ClaStorage { base: base.clone() };
        let user_approval_storage = UserApprovalStorage { base: base.clone() };
        let user_storage = UserStorage { base: base.clone() };
        let group_storage = GroupStorage { base: base.clone() };
        let cl_storage = ClStorage { base: base.clone() };
        let issue_storage = IssueStorage { base: base.clone() };
        let vault_storage = VaultStorage { base: base.clone() };
        let conversation_storage = ConversationStorage { base: base.clone() };
        let object_store = ObjectStorageFactory::build(&config.object_storage).await?;
        let lfs_service = LfsService {
            lfs_storage: lfs_db_storage.clone(),
            obj_storage: object_store.clone(),
        };

        let note_storage = NoteStorage { base: base.clone() };
        let commit_binding_storage = CommitBindingStorage { base: base.clone() };
        let reviewer_storage = ClReviewerStorage { base: base.clone() };
        let merge_queue_storage = MergeQueueStorage::new(base.clone());
        let buck_storage = BuckStorage { base: base.clone() };

        let dynamic_sidebar_storage = DynamicSidebarStorage { base: base.clone() };
        dynamic_sidebar_storage
            .init_default_sidebars(&config.sidebar)
            .await?;

        let code_review_comment_storage = CodeReviewCommentStorage { base: base.clone() };
        let code_review_thread_storage = CodeReviewThreadStorage { base: base.clone() };
        let build_trigger_storage = BuildTriggerStorage { base: base.clone() };
        let bots_storage = BotsStorage { base: base.clone() };
        let webhook_storage = WebhookStorage { base: base.clone() };
        let audit_storage = AuditStorage { base: base.clone() };

        let git_service = GitService {
            obj_storage: object_store.clone(),
        };
        let mono_service = MonoService {
            mono_storage: mono_storage.clone(),
            git_service: git_service.clone(),
        };

        let import_service = ImportService {
            git_db_storage: git_db_storage.clone(),
            git_service: git_service.clone(),
        };

        let buck_config = config.buck.clone().unwrap_or_default();

        // Validate configuration
        if let Err(e) = buck_config.validate() {
            let error_msg = format!(
                "Invalid Buck configuration: {}. Service cannot start with invalid configuration.",
                e
            );
            tracing::error!("{}", error_msg);
            panic!("{}", error_msg);
        }

        let upload_semaphore = Arc::new(Semaphore::new(
            buck_config.upload_concurrency_limit as usize,
        ));
        let large_file_semaphore = Arc::new(Semaphore::new(
            buck_config.large_file_concurrency_limit as usize,
        ));

        let app_service = AppService {
            mono_storage: mono_storage.clone(),
            git_db_storage,
            gpg_storage,
            lfs_db_storage,
            cla_storage,
            user_approval_storage,
            user_storage,
            group_storage,
            vault_storage,
            cl_storage: cl_storage.clone(),
            issue_storage,
            conversation_storage,
            note_storage,
            commit_binding_storage,
            reviewer_storage,
            merge_queue_storage: merge_queue_storage.clone(),
            buck_storage,
            dynamic_sidebar_storage,
            code_review_comment_storage,
            code_review_thread_storage,
            build_trigger_storage,
            bots_storage,
            webhook_storage: webhook_storage.clone(),
            audit_storage,
            notification_storage,
        };
        let merge_queue_service = MergeQueueService::new(base.clone());
        let artifact_service = ArtifactService::new(base.clone(), object_store.clone());
        let buck_service = BuckService::new(
            base.clone(),
            CLService::new(base.clone()),
            upload_semaphore,
            large_file_semaphore,
            buck_config,
            git_service.clone(),
        )
        .expect("failed to create BuckService");

        let webhook_service = WebhookService::new(webhook_storage.clone())?;

        Ok(Storage {
            app_service: app_service.into(),
            cla_service: ClaService::new(base.clone()),
            config: Arc::downgrade(&config),
            issue_service: IssueService::new(base.clone()),
            cl_service: CLService::new(base.clone()),
            merge_queue_service,
            artifact_service,
            buck_service,
            git_service,
            mono_service,
            import_service,
            lfs_service,
            code_review_service: CodeReviewService::new(base.clone()),
            webhook_service,
        })
    }

    pub fn config(&self) -> Arc<Config> {
        self.config.upgrade().expect("Config has been dropped")
    }

    /// Get recommended concurrency limit for batch database operations.
    ///
    /// Calculates 50% of max_connection, bounded between 4 and max_connection.
    pub fn get_recommended_batch_concurrency(&self) -> usize {
        let max_conn = self.config().database.max_connection as usize;

        // Handle edge case where config might be 0 or invalid
        let safe_conn = if max_conn == 0 { 16 } else { max_conn };

        // Internal calculation using pure function
        calculate_db_concurrency_limit(safe_conn, 50, 4)
    }

    pub fn mono_storage(&self) -> MonoStorage {
        self.app_service.mono_storage.clone()
    }

    /// Begin a database transaction on the shared app connection (monorepo + import metadata).
    pub async fn begin_db_transaction(&self) -> Result<sea_orm::DatabaseTransaction, MegaError> {
        use sea_orm::TransactionTrait;
        self.mono_storage()
            .get_connection()
            .begin()
            .await
            .map_err(MegaError::Db)
    }

    /// Best-effort classification/logging helper for object storage "not found" errors
    /// when dealing with Git blobs.
    ///
    /// Behavior:
    /// - If `err` is not `MegaError::ObjStorageNotFound`, it is returned unchanged.
    /// - If it is `ObjStorageNotFound(msg)`, we:
    ///   - Look up blob metadata via `mega_blob` table
    ///   - Emit structured logs to distinguish:
    ///       - "missing_in_db_and_s3" (likely never written / invalid request)
    ///       - "missing_in_s3_but_has_meta" (data loss or external deletion)
    ///   - Return a new `MegaError::ObjStorageNotFound` whose message is prefixed with
    ///     a classification tag:
    ///       - "[obj_missing_in_db_and_s3] ..."
    ///       - "[obj_missing_in_s3_but_has_meta] ..."
    ///       - "[obj_meta_lookup_failed] ..." (if metadata lookup itself fails)
    pub async fn classify_blob_objstorage_not_found(
        &self,
        hash: &str,
        err: MegaError,
    ) -> MegaError {
        match err {
            MegaError::ObjStorageNotFound(_msg) => {
                let mono_storage = self.mono_storage();

                match mono_storage
                    .get_mega_blobs_by_hashes(vec![hash.to_string()])
                    .await
                {
                    Ok(blobs) if blobs.is_empty() => {
                        let friendly = format!(
                            "[obj_missing_in_db_and_s3] Blob {hash} not found in both object storage and metadata (likely never written or invalid request)"
                        );
                        tracing::warn!("{}", friendly);
                        MegaError::ObjStorageNotFound(friendly)
                    }
                    Ok(mut blobs) => {
                        let blob = blobs.pop().expect(
                            "blobs is guaranteed non-empty here due to match guard on previous arm",
                        );
                        tracing::error!(
                            "[obj_missing_in_s3_but_has_meta] Object with hash {hash} missing in S3 but metadata exists in DB for blob_id {}; possible data loss or misconfiguration",
                            blob.blob_id,
                        );
                        MegaError::ObjStorageInconsistent(format!(
                            "[obj_missing_in_s3_but_has_meta] Object {hash} missing in S3 but metadata exists; possible data loss or misconfiguration"
                        ))
                    }
                    Err(_) => MegaError::ObjStorageInconsistent(format!(
                        "[obj_meta_lookup_failed] Failed to query blob {hash} metadata while handling ObjStorageNotFound"
                    )),
                }
            }
            other => other,
        }
    }

    pub fn git_db_storage(&self) -> GitDbStorage {
        self.app_service.git_db_storage.clone()
    }

    pub fn gpg_storage(&self) -> GpgStorage {
        self.app_service.gpg_storage.clone()
    }

    pub fn lfs_db_storage(&self) -> LfsDbStorage {
        self.app_service.lfs_db_storage.clone()
    }

    pub fn user_storage(&self) -> UserStorage {
        self.app_service.user_storage.clone()
    }

    pub fn cla_storage(&self) -> ClaStorage {
        self.app_service.cla_storage.clone()
    }

    pub fn user_approval_storage(&self) -> UserApprovalStorage {
        self.app_service.user_approval_storage.clone()
    }

    pub fn group_storage(&self) -> GroupStorage {
        self.app_service.group_storage.clone()
    }

    pub fn vault_storage(&self) -> VaultStorage {
        self.app_service.vault_storage.clone()
    }

    pub fn cl_storage(&self) -> ClStorage {
        self.app_service.cl_storage.clone()
    }

    pub fn issue_storage(&self) -> IssueStorage {
        self.app_service.issue_storage.clone()
    }

    pub fn conversation_storage(&self) -> ConversationStorage {
        self.app_service.conversation_storage.clone()
    }

    pub fn note_storage(&self) -> NoteStorage {
        self.app_service.note_storage.clone()
    }

    pub fn commit_binding_storage(&self) -> CommitBindingStorage {
        self.app_service.commit_binding_storage.clone()
    }

    pub fn reviewer_storage(&self) -> ClReviewerStorage {
        self.app_service.reviewer_storage.clone()
    }

    pub fn merge_queue_storage(&self) -> MergeQueueStorage {
        self.app_service.merge_queue_storage.clone()
    }

    pub fn buck_storage(&self) -> BuckStorage {
        self.app_service.buck_storage.clone()
    }

    pub fn dynamic_sidebar_storage(&self) -> DynamicSidebarStorage {
        self.app_service.dynamic_sidebar_storage.clone()
    }

    pub fn code_review_thread_storage(&self) -> CodeReviewThreadStorage {
        self.app_service.code_review_thread_storage.clone()
    }

    pub fn code_review_comment_storage(&self) -> CodeReviewCommentStorage {
        self.app_service.code_review_comment_storage.clone()
    }

    pub fn build_trigger_storage(&self) -> BuildTriggerStorage {
        self.app_service.build_trigger_storage.clone()
    }

    pub fn webhook_storage(&self) -> WebhookStorage {
        self.app_service.webhook_storage.clone()
    }

    pub fn audit_storage(&self) -> AuditStorage {
        self.app_service.audit_storage.clone()
    }

    pub fn bots_storage(&self) -> BotsStorage {
        self.app_service.bots_storage.clone()
    }

    pub fn notification_storage(&self) -> NotificationStorage {
        self.app_service.notification_storage.clone()
    }

    pub fn mock() -> Self {
        // During test time, we don't need a AppContext,
        // Put config in a leaked static variable thus the weak reference will always be valid.
        static CONFIG: LazyLock<Arc<Config>> = LazyLock::new(|| Config::mock().into());

        let app_service = AppService::mock();
        let webhook_service = WebhookService::mock(app_service.webhook_storage.clone());

        Storage {
            app_service,
            // app_service: AppService::mock(),
            cla_service: ClaService::mock(),
            issue_service: IssueService::mock(),
            cl_service: CLService::mock(),
            merge_queue_service: MergeQueueService::mock(),
            artifact_service: ArtifactService::mock(),
            buck_service: BuckService::mock(),
            config: Arc::downgrade(&*CONFIG),
            git_service: GitService::mock(),
            mono_service: MonoService::mock(),
            import_service: ImportService::mock(),
            lfs_service: LfsService::mock(),
            code_review_service: CodeReviewService::mock(),
            webhook_service,
        }
    }
}

// Private helper function for concurrency calculation
// This is a pure function for easy testing and potential reuse
fn calculate_db_concurrency_limit(
    max_connections: usize,
    percentage: usize,
    min_limit: usize,
) -> usize {
    let calculated = (max_connections * percentage) / 100;
    calculated.max(min_limit).min(max_connections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_db_concurrency_limit() {
        // Normal case: 50% of 16 = 8
        assert_eq!(calculate_db_concurrency_limit(16, 50, 4), 8);

        // Min bound: 50% of 5 = 2.5 -> clamped to min 4
        assert_eq!(calculate_db_concurrency_limit(5, 50, 4), 4);

        // Max bound: 50% of 100 = 50 (within bounds)
        assert_eq!(calculate_db_concurrency_limit(100, 50, 4), 50);

        // Edge case: small connection pool
        assert_eq!(calculate_db_concurrency_limit(4, 50, 4), 4);

        // Edge case: very small connection pool (cannot exceed max_connections)
        assert_eq!(calculate_db_concurrency_limit(2, 50, 4), 2);
    }

    #[test]
    fn test_get_recommended_batch_concurrency() {
        // Create a mock Storage for testing
        let storage = Storage::mock();

        // The mock config should have default max_connection = 16
        let concurrency = storage.get_recommended_batch_concurrency();

        // Should be 50% of 16 = 8
        assert_eq!(concurrency, 8);
    }
}
