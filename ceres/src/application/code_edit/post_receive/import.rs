//! Import repo attach-to-monorepo handler.

use std::{
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Instant,
};

use callisto::sea_orm_active_enums::RefTypeEnum;
use common::errors::MegaError;
use git_internal::{hash::ObjectHash, internal::object::commit::Commit};
use jupiter::{redis::lock::RedLock, storage::Storage, utils::converter::FromGitModel};

use crate::{
    application::api_service::{cache::GitObjectCache, mono::MonoApiService, tree_ops},
    transport::protocol::import_refs::{CommandType, RefCommand},
};

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_import_receive_pack_finalized(
    storage: Storage,
    _git_object_cache: Arc<GitObjectCache>,
    mono_api_service: &MonoApiService,
    repo_path: PathBuf,
    repo_id: i64,
    commands: Vec<RefCommand>,
    unpack_redlock: Arc<RedLock>,
    extra_timings: Arc<Mutex<Vec<(String, u128)>>>,
) -> Result<(), MegaError> {
    let commit_id = match commands.iter().find(|c| c.ref_type == RefTypeEnum::Branch) {
        Some(cmd) => cmd.new_id.clone(),
        None => return Ok(()),
    };

    let mono_storage = storage.mono_storage();

    let latest_commit: Commit = Commit::from_git_model(
        storage
            .git_db_storage()
            .get_commit_by_hash(repo_id, &commit_id)
            .await?
            .ok_or_else(|| MegaError::Other(format!("commit {commit_id} not found")))?,
    );
    let commit_msg = latest_commit.format_message();

    const MAX_ATTACH_ATTEMPTS: u32 = 64;
    let mut root_lock_wait_max_ms: u128 = 0;
    let mut root_lock_wait_sum_ms: u128 = 0;

    for attempt in 0..MAX_ATTACH_ATTEMPTS {
        let t_lock = Instant::now();
        let guard = unpack_redlock.clone().lock().await?;
        let lock_wait_ms = t_lock.elapsed().as_millis();
        root_lock_wait_max_ms = root_lock_wait_max_ms.max(lock_wait_ms);
        root_lock_wait_sum_ms += lock_wait_ms;

        let root_ref = mono_storage
            .get_main_ref("/")
            .await?
            .ok_or_else(|| MegaError::Other("root ref not found".to_string()))?;
        let expected_commit = root_ref.ref_commit_hash.clone();
        let expected_tree = root_ref.ref_tree_hash.clone();
        let root_ref_id = root_ref.id;

        let (save_trees, gitkeep_blob) =
            tree_ops::search_and_create_tree(mono_api_service, &repo_path).await?;

        let new_commit = Commit::from_tree_id(
            save_trees
                .back()
                .ok_or_else(|| MegaError::Other("no tree generated".to_string()))?
                .id,
            vec![ObjectHash::from_str(&expected_commit).unwrap()],
            &format!("\n{commit_msg}"),
        );

        // Persist placeholder .gitkeep referenced by newly created path trees.
        storage
            .mono_service
            .save_blobs(&new_commit.id.to_string(), vec![gitkeep_blob])
            .await?;

        let txn = storage.begin_db_transaction().await?;
        let git_db = storage.git_db_storage();
        for cmd in &commands {
            if cmd.ref_type != RefTypeEnum::Branch {
                continue;
            }
            match cmd.command_type {
                CommandType::Create => {
                    git_db
                        .save_ref_in_txn(repo_id, cmd.clone().into(), &txn)
                        .await?;
                }
                CommandType::Delete => {
                    git_db
                        .remove_ref_in_txn(repo_id, &cmd.ref_name, &txn)
                        .await?;
                }
                CommandType::Update => {
                    git_db
                        .update_ref_in_txn(repo_id, &cmd.ref_name, &cmd.new_id, &txn)
                        .await?;
                }
            }
        }

        let t_attach_txn = Instant::now();
        match mono_storage
            .attach_to_monorepo_parent_in_txn(
                &txn,
                root_ref_id,
                &expected_commit,
                &expected_tree,
                new_commit,
                save_trees.into(),
            )
            .await
        {
            Ok(()) => {
                txn.commit().await.map_err(MegaError::Db)?;
                let t_unlock = Instant::now();
                guard.unlock().await?;
                extra_timings
                    .lock()
                    .expect("import extra_timings lock poisoned")
                    .extend([
                        (
                            "import_attach_attempts_count".to_string(),
                            (attempt + 1) as u128,
                        ),
                        (
                            "import_root_lock_wait_sum_ms".to_string(),
                            root_lock_wait_sum_ms,
                        ),
                        (
                            "import_root_lock_wait_max_ms".to_string(),
                            root_lock_wait_max_ms,
                        ),
                        (
                            "import_attach_txn_ms".to_string(),
                            t_attach_txn.elapsed().as_millis(),
                        ),
                        (
                            "import_root_lock_unlock_ms".to_string(),
                            t_unlock.elapsed().as_millis(),
                        ),
                    ]);
                return Ok(());
            }
            Err(MegaError::StaleMonorepoRootRef) if attempt + 1 < MAX_ATTACH_ATTEMPTS => {
                let _ = txn.rollback().await;
                let _ = guard.unlock().await;
                tracing::warn!(
                    attempt = attempt,
                    repo_path = %repo_path.display(),
                    "attach_to_monorepo_parent: root ref moved, retrying"
                );
                tokio::task::yield_now().await;
            }
            Err(e) => {
                let _ = txn.rollback().await;
                let _ = guard.unlock().await;
                extra_timings
                    .lock()
                    .expect("import extra_timings lock poisoned")
                    .extend([
                        (
                            "import_attach_attempts_count".to_string(),
                            (attempt + 1) as u128,
                        ),
                        (
                            "import_root_lock_wait_sum_ms".to_string(),
                            root_lock_wait_sum_ms,
                        ),
                        (
                            "import_root_lock_wait_max_ms".to_string(),
                            root_lock_wait_max_ms,
                        ),
                        (
                            "import_attach_txn_ms".to_string(),
                            t_attach_txn.elapsed().as_millis(),
                        ),
                    ]);
                return Err(e);
            }
        }
    }

    Err(MegaError::Other(
        "attach_to_monorepo_parent: exceeded retry limit for concurrent root updates".into(),
    ))
}
