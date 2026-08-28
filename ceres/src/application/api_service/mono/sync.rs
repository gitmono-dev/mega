//! Third-party sync and tree traversal for [`MonoApiService`](super::service::MonoApiService).

use std::{path::PathBuf, str::FromStr, sync::Arc};

use bytes::Bytes;
use common::{errors::MegaError, utils::ZERO_ID};
use git_internal::{
    hash::ObjectHash,
    internal::object::{commit::Commit, tree::Tree},
};
use jupiter::{redis::lock::RedLock, utils::converter::FromMegaModel};

use crate::{
    application::api_service::{
        mono::{MonoApiService, MonoServiceLogic},
        tree_ops,
    },
    bus::TransportRuntime,
    infra::pack_stream::into_pack_byte_stream,
    model::third_party::{ThirdPartyClient, ThirdPartyRepoTrait},
    transport::protocol::{PushUserInfo, ServiceType, SmartSession, TransportProtocol},
};

impl MonoApiService {
    /// Attach a new `/project/*` path into the monorepo root tree (placeholder `.gitkeep` dirs).
    pub async fn attach_project_path_to_monorepo_root(&self, path: &str) -> Result<(), MegaError> {
        const MAX_ATTACH_ATTEMPTS: u32 = 64;
        const ROOT_LOCK_KEY: &str = "git:receive-pack:lock:monorepo-root";
        const ROOT_LOCK_TTL_MS: u64 = 30_000;

        let path_buf = PathBuf::from(path);
        let storage = self.storage().mono_storage();
        let redlock = Arc::new(RedLock::new(
            self.git_object_cache().connection.clone(),
            ROOT_LOCK_KEY.to_string(),
            ROOT_LOCK_TTL_MS,
        ));

        for attempt in 0..MAX_ATTACH_ATTEMPTS {
            // Same split as import receive-pack attach: walk/upload off the
            // global root lock; CAS on mega_refs still serializes the root update.
            let root_ref = storage
                .get_main_ref("/")
                .await?
                .ok_or_else(|| MegaError::Other("root ref not found".to_string()))?;
            let expected_commit = root_ref.ref_commit_hash.clone();
            let expected_tree = root_ref.ref_tree_hash.clone();
            let root_ref_id = root_ref.id;

            let (save_trees, gitkeep_blob) =
                tree_ops::search_and_create_tree(self, &path_buf).await?;
            let leaf_tree = save_trees
                .back()
                .ok_or_else(|| MegaError::Other("no tree generated".to_string()))?;
            let commit_msg = format!("\nInitialize path {path} for project GitHub sync");
            let new_commit = Commit::from_tree_id(
                leaf_tree.id,
                vec![
                    ObjectHash::from_str(&expected_commit)
                        .map_err(|e| MegaError::Other(format!("invalid root commit hash: {e}")))?,
                ],
                &commit_msg,
            );

            // Persist .gitkeep before attaching trees. Previously only trees/commits
            // were saved, leaving object storage without the blob and breaking clone.
            self.storage()
                .mono_service
                .save_blobs(&new_commit.id.to_string(), vec![gitkeep_blob])
                .await?;

            let guard = redlock.clone().lock().await?;
            let current_root = storage
                .get_main_ref("/")
                .await?
                .ok_or_else(|| MegaError::Other("root ref not found".to_string()))?;
            if current_root.ref_commit_hash != expected_commit
                || current_root.ref_tree_hash != expected_tree
                || current_root.id != root_ref_id
            {
                let _ = guard.unlock().await;
                tracing::warn!(
                    attempt,
                    repo_path = %path,
                    "attach_project_path_to_monorepo_root: root ref moved before txn, retrying"
                );
                continue;
            }

            let txn = self.storage().begin_db_transaction().await?;
            match storage
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
                    guard.unlock().await?;
                    crate::application::api_service::mono::cl_merge::sync_path_prefix_main_refs(
                        self, path,
                    )
                    .await?;
                    return Ok(());
                }
                Err(MegaError::StaleMonorepoRootRef) if attempt + 1 < MAX_ATTACH_ATTEMPTS => {
                    let _ = txn.rollback().await;
                    let _ = guard.unlock().await;
                    tracing::warn!(
                        attempt,
                        repo_path = %path,
                        "attach_project_path_to_monorepo_root: root ref moved, retrying"
                    );
                    tokio::task::yield_now().await;
                }
                Err(e) => {
                    let _ = txn.rollback().await;
                    let _ = guard.unlock().await;
                    return Err(e);
                }
            }
        }

        Err(MegaError::Other(
            "attach_project_path_to_monorepo_root: exceeded retry limit".into(),
        ))
    }

    async fn resolve_sync_old_id(
        &self,
        repo_path_str: &str,
        ref_name: &str,
    ) -> Result<String, MegaError> {
        let import_dir = self.storage().config().monorepo.import_dir.clone();
        if PathBuf::from(repo_path_str).starts_with(&import_dir) {
            let storage = self.storage().git_db_storage();
            match storage.find_git_repo_exact_match(repo_path_str).await? {
                Some(repo_model) => Ok(storage
                    .get_ref(repo_model.id)
                    .await?
                    .into_iter()
                    .find(|r| r.ref_name == ref_name)
                    .map(|r| r.ref_git_id)
                    .unwrap_or_else(|| ZERO_ID.to_string())),
                None => Ok(ZERO_ID.to_string()),
            }
        } else {
            let mono_storage = self.storage().mono_storage();
            if let Some(r) = mono_storage
                .get_ref_at_path(repo_path_str, ref_name)
                .await?
            {
                return Ok(r.ref_commit_hash);
            }
            // GitHub sync stores commits on CL refs (`refs/cl/*`), not `refs/heads/main`.
            if let Some(cl_ref) = mono_storage
                .get_all_refs(repo_path_str, false)
                .await?
                .into_iter()
                .find(|r| r.is_cl)
            {
                return Ok(cl_ref.ref_commit_hash);
            }
            Ok(ZERO_ID.to_string())
        }
    }

    pub async fn sync_third_party_repo(
        &self,
        transport: &TransportRuntime,
        owner: &str,
        repo: &str,
        mega_path: PathBuf,
        username: &str,
    ) -> Result<Bytes, MegaError> {
        let repo_path_str = mega_path
            .to_str()
            .ok_or_else(|| MegaError::Other("Invalid UTF-8 in mega_path".to_string()))?;
        let repo_path_str = MonoServiceLogic::validate_github_sync_path(repo_path_str)?;
        let mega_path = PathBuf::from(&repo_path_str);

        let url = format!("https://github.com/{owner}/{repo}.git");
        let remote_client = ThirdPartyClient::new(&url);

        let import_dir = self.storage().config().monorepo.import_dir.clone();
        let fetch_depth = if mega_path.starts_with(&import_dir) {
            None
        } else {
            // MonoRepo receive-pack only accepts a single commit per push.
            Some(1)
        };

        let (ref_name, ref_hash) = remote_client.fetch_refs().await?;

        let res = remote_client
            .fetch_packs(std::slice::from_ref(&ref_hash), fetch_depth)
            .await?;
        let pack_data = remote_client.process_pack_stream(res).await?;
        if pack_data.is_empty() {
            return Err(MegaError::Other(
                "GitHub sync failed: remote returned no pack data".to_string(),
            ));
        }

        let mut protocol =
            SmartSession::new(mega_path, ServiceType::ReceivePack, TransportProtocol::Http);
        protocol.auth.username = Some(username.to_string());
        protocol.auth.authenticated_user = Some(PushUserInfo {
            username: username.to_string(),
        });

        let old_id = self.resolve_sync_old_id(&repo_path_str, &ref_name).await?;

        let commands = vec![crate::transport::protocol::import_refs::RefCommand::new(
            old_id,
            ref_hash.clone(),
            ref_name.clone(),
        )];
        let state = transport.clone();
        let bytes = protocol
            .git_receive_pack_stream(
                &state,
                commands,
                Some(into_pack_byte_stream(tokio_stream::once(Ok::<
                    Bytes,
                    std::convert::Infallible,
                >(
                    Bytes::from(pack_data),
                )))),
            )
            .await
            .map_err(|e| MegaError::Other(format!("{e}")))?;

        if MonoServiceLogic::receive_pack_report_failed(&bytes) {
            return Err(MegaError::Other(format!(
                "GitHub sync failed during receive-pack: {}",
                String::from_utf8_lossy(&bytes)
            )));
        }

        Ok(bytes)
    }

    pub(crate) async fn traverse_tree(
        &self,
        root_tree: Tree,
    ) -> Result<Vec<(PathBuf, ObjectHash)>, MegaError> {
        let mut result = vec![];
        let mut stack = vec![(PathBuf::new(), root_tree)];

        while let Some((base_path, tree)) = stack.pop() {
            for item in tree.tree_items {
                let path = base_path.join(&item.name);
                if item.is_tree() {
                    let child = self
                        .storage()
                        .mono_storage()
                        .get_tree_by_hash(&item.id.to_string())
                        .await?
                        .unwrap();
                    stack.push((path.clone(), Tree::from_mega_model(child)));
                } else {
                    result.push((path, item.id));
                }
            }
        }
        Ok(result)
    }
}
