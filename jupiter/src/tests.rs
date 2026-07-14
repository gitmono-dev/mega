use std::{
    path::Path,
    sync::{Arc, LazyLock},
};

use common::config::Config;
use io_orbit::factory::MegaObjectStorageWrapper;
#[cfg(feature = "migrate")]
use jupiter_migrate::apply_migrations;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tracing::log;

use crate::{
    service::{
        artifact_service::ArtifactService, buck_service::BuckService, cl_service::CLService,
        cla_service::ClaService, code_review_service::CodeReviewService, git_service::GitService,
        import_service::ImportService, issue_service::IssueService, lfs_service::LfsService,
        merge_queue_service::MergeQueueService, mono_service::MonoService,
        webhook_service::WebhookService,
    },
    storage::{
        AppService, Storage,
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
        issue_storage::IssueStorage,
        lfs_db_storage::LfsDbStorage,
        merge_queue_storage::MergeQueueStorage,
        mono_storage::MonoStorage,
        note_storage::NoteStorage,
        notification_storage::NotificationStorage,
        user_approval_storage::UserApprovalStorage,
        user_storage::UserStorage,
        vault_storage::VaultStorage,
        webhook_storage::WebhookStorage,
    },
};

pub async fn test_db_connection(temp_dir: &Path) -> DatabaseConnection {
    let db_url = format!("sqlite://{}/test.db", temp_dir.to_string_lossy());
    std::fs::File::create(temp_dir.join("test.db")).expect("Failed to create test database file");

    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(5)
        .min_connections(1)
        .sqlx_logging(true)
        .sqlx_logging_level(log::LevelFilter::Debug);

    Database::connect(opt)
        .await
        .expect("Failed to connect to mock database")
}

pub async fn test_storage(temp_dir: impl AsRef<Path>) -> Storage {
    static CONFIG: LazyLock<Arc<Config>> = LazyLock::new(|| Config::mock().into());
    let connection = test_db_connection(temp_dir.as_ref()).await;
    let connection = Arc::new(connection);
    let config = CONFIG.clone();
    let base = BaseStorage::new(connection.clone());

    let svc = AppService {
        mono_storage: MonoStorage { base: base.clone() },
        git_db_storage: GitDbStorage { base: base.clone() },
        gpg_storage: GpgStorage { base: base.clone() },
        lfs_db_storage: LfsDbStorage { base: base.clone() },
        cla_storage: ClaStorage { base: base.clone() },
        user_approval_storage: UserApprovalStorage { base: base.clone() },
        user_storage: UserStorage { base: base.clone() },
        group_storage: GroupStorage { base: base.clone() },
        cl_storage: ClStorage { base: base.clone() },
        issue_storage: IssueStorage { base: base.clone() },
        vault_storage: VaultStorage { base: base.clone() },
        conversation_storage: ConversationStorage { base: base.clone() },
        note_storage: NoteStorage { base: base.clone() },
        commit_binding_storage: CommitBindingStorage { base: base.clone() },
        reviewer_storage: ClReviewerStorage { base: base.clone() },
        merge_queue_storage: MergeQueueStorage::new(base.clone()),
        buck_storage: BuckStorage { base: base.clone() },
        dynamic_sidebar_storage: DynamicSidebarStorage { base: base.clone() },
        code_review_comment_storage: CodeReviewCommentStorage { base: base.clone() },
        code_review_thread_storage: CodeReviewThreadStorage { base: base.clone() },
        build_trigger_storage: BuildTriggerStorage { base: base.clone() },
        notification_storage: NotificationStorage::new(connection.clone()),
        bots_storage: BotsStorage { base: base.clone() },
        webhook_storage: WebhookStorage { base: base.clone() },
        audit_storage: AuditStorage { base: base.clone() },
    };

    #[cfg(feature = "migrate")]
    apply_migrations(&connection, true).await.unwrap();

    let webhook_service = WebhookService::mock(svc.webhook_storage.clone());

    Storage {
        app_service: Arc::new(svc),
        cla_service: ClaService::new(base.clone()),
        issue_service: IssueService::mock(),
        cl_service: CLService::mock(),
        merge_queue_service: MergeQueueService::mock(),
        artifact_service: ArtifactService::new(base.clone(), MegaObjectStorageWrapper::mock()),
        buck_service: BuckService::mock(),
        config: Arc::downgrade(&config),
        git_service: GitService::mock(),
        mono_service: MonoService::mock(),
        import_service: ImportService::mock(),
        lfs_service: LfsService::mock(),
        code_review_service: CodeReviewService::mock(),
        webhook_service,
    }
}
