//! Buck upload operations for [`MonoApiService`](super::service::MonoApiService).

use std::{collections::HashMap, path::PathBuf};

use callisto;
use common::errors::{BuckError, MegaError};
use jupiter::{
    service::buck_service::{
        CommitArtifacts, CompletePayload as SvcCompletePayload,
        CompleteResponse as SvcCompleteResponse,
    },
    storage::buck_storage::{session_status, upload_status},
    utils::converter::IntoMegaModel,
};

use crate::{
    application::{
        api_service::{
            buck_tree_builder::BuckCommitBuilder,
            mono::{MonoApiService, MonoServiceLogic},
        },
        build_trigger::{BuildTriggerService, TriggerContext},
    },
    model::buck::{
        CompletePayload, CompleteResponse, DEFAULT_MODE, FileChange,
        FileToUpload as ApiFileToUpload, ManifestPayload, ManifestResponse,
    },
};

impl MonoApiService {
    /// Triggers a build for Buck upload completion
    fn trigger_build_for_buck_upload(&self, response: &CompleteResponse, username: &str) {
        let Some(build_dispatch) = self.build_dispatch() else {
            return;
        };
        if !build_dispatch.enable_build() {
            return;
        }
        let storage = self.storage().clone();
        let git_cache = self.git_object_cache().clone();
        let mut context = TriggerContext::from_buck_upload(
            response.repo_path.clone(),
            response.from_hash.clone(),
            response.commit_id.clone(),
            response.cl_link.clone(),
            Some(response.cl_id),
            Some(response.repo_path.clone()),
            Some(username.to_string()),
        );
        context.ref_name = Some("main".to_string());
        context.ref_type = Some("branch".to_string());
        tokio::spawn(async move {
            if let Err(e) =
                BuildTriggerService::build_by_context(storage, git_cache, build_dispatch, context)
                    .await
            {
                tracing::error!("Failed to create build trigger for buck upload: {}", e);
            }
        });
    }
    pub async fn create_buck_session(
        &self,
        username: &str,
        path: &str,
    ) -> Result<jupiter::service::buck_service::SessionResponse, MegaError> {
        let path = path.trim();
        if path.is_empty() {
            return Err(MegaError::bad_request("Path cannot be empty"));
        }
        let normalized_path = MonoServiceLogic::normalize_repo_path(path)?;
        let refs = self
            .storage()
            .mono_storage()
            .get_main_ref(&normalized_path)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("Path not found: {}", normalized_path)))?;
        let base_branch = refs
            .ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(refs.ref_name.as_str())
            .to_string();
        // Use canonical path from mega_refs as the single source of truth for repository path
        let canonical_path = refs.path.clone();
        let response = self
            .storage()
            .buck_service
            .create_session(
                username,
                &canonical_path,
                &base_branch,
                refs.ref_commit_hash,
            )
            .await?;

        Ok(response)
    }

    /// Process buck upload manifest.
    ///
    /// # Arguments
    /// * `username` - User processing the manifest
    /// * `cl_link` - CL link
    /// * `payload` - Manifest payload
    ///
    /// # Returns
    /// Returns `ManifestResponse` on success
    pub async fn process_buck_manifest(
        &self,
        username: &str,
        cl_link: &str,
        payload: ManifestPayload,
    ) -> Result<ManifestResponse, MegaError> {
        let session = self
            .storage()
            .buck_storage()
            .get_session(cl_link)
            .await?
            .ok_or_else(|| MegaError::Buck(BuckError::SessionNotFound(cl_link.to_string())))?;

        if session.user_id != username {
            return Err(MegaError::Buck(BuckError::Forbidden(
                "Session belongs to another user".to_string(),
            )));
        }

        let manifest_paths: Vec<PathBuf> = payload
            .files
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect();

        // Get content hashes (raw SHA-1) and blob IDs
        let (existing_file_hashes, existing_blob_ids_map) =
            crate::application::api_service::blob_ops::get_files_content_hashes_with_blob_ids(
                self,
                &manifest_paths,
                session.from_hash.as_deref(),
            )
            .await
            .map_err(MegaError::Git)?;

        // Convert ObjectHash to String for storage
        let existing_blob_ids: HashMap<PathBuf, String> = existing_blob_ids_map
            .into_iter()
            .map(|(path, blob_hash)| (path, blob_hash.to_string()))
            .collect();

        // Convert payload to service layer type
        let service_payload = jupiter::service::buck_service::ManifestPayload {
            files: payload
                .files
                .iter()
                .map(|f| jupiter::service::buck_service::ManifestFile {
                    path: f.path.clone(),
                    size: f.size,
                    hash: f.hash.clone(),
                })
                .collect(),
            commit_message: payload.commit_message.clone(),
        };

        let svc_resp = self
            .storage()
            .buck_service
            .process_manifest(
                username,
                cl_link,
                service_payload,
                existing_file_hashes,
                existing_blob_ids,
            )
            .await?;

        // Convert back to API layer response
        let api_resp = ManifestResponse {
            total_files: svc_resp.total_files,
            total_size: svc_resp.total_size,
            files_to_upload: svc_resp
                .files_to_upload
                .into_iter()
                .map(|f| ApiFileToUpload {
                    path: f.path,
                    reason: f.reason,
                })
                .collect(),
            files_unchanged: svc_resp.files_unchanged,
            upload_size: svc_resp.upload_size,
        };

        Ok(api_resp)
    }

    pub fn buck_max_file_size(&self) -> u64 {
        self.storage().buck_service.max_file_size()
    }

    pub fn buck_try_acquire_upload_permits(
        &self,
        file_size: u64,
    ) -> Result<
        (
            tokio::sync::OwnedSemaphorePermit,
            Option<tokio::sync::OwnedSemaphorePermit>,
        ),
        MegaError,
    > {
        self.storage()
            .buck_service
            .try_acquire_upload_permits(file_size)
    }

    pub async fn upload_buck_file(
        &self,
        username: &str,
        cl_link: &str,
        file_path: &str,
        file_size: u64,
        file_hash: Option<&str>,
        file_content: bytes::Bytes,
    ) -> Result<jupiter::service::buck_service::FileUploadResponse, MegaError> {
        self.storage()
            .buck_service
            .upload_file(
                username,
                cl_link,
                file_path,
                file_size,
                file_hash,
                file_content,
            )
            .await
    }

    /// Complete buck upload.
    ///
    /// Commit message is read from session.commit_message which is set during Manifest phase.
    /// The payload is intentionally unused (empty struct).
    ///
    /// # Arguments
    /// * `username` - User completing the upload
    /// * `cl_link` - CL link
    /// * `_payload` - Empty payload (unused). Commit message is read from session.commit_message
    ///   which is set during Manifest phase.
    ///
    /// # Returns
    /// Returns `CompleteResponse` on success
    pub async fn complete_buck_upload(
        &self,
        username: &str,
        cl_link: &str,
        _payload: CompletePayload,
    ) -> Result<CompleteResponse, MegaError> {
        let session = self
            .storage()
            .buck_storage()
            .get_session(cl_link)
            .await?
            .ok_or_else(|| MegaError::Buck(BuckError::SessionNotFound(cl_link.to_string())))?;

        if session.user_id != username {
            return Err(MegaError::Buck(BuckError::Forbidden(
                "Session belongs to another user".to_string(),
            )));
        }

        if ![session_status::MANIFEST_UPLOADED, session_status::UPLOADING]
            .contains(&session.status.as_str())
        {
            return Err(MegaError::Buck(BuckError::InvalidSessionStatus {
                expected: format!(
                    "{} or {}",
                    session_status::MANIFEST_UPLOADED,
                    session_status::UPLOADING
                ),
                actual: session.status.clone(),
            }));
        }

        let pending = self
            .storage()
            .buck_storage()
            .count_pending_files(cl_link)
            .await?;
        if pending > 0 {
            return Err(MegaError::Buck(BuckError::FilesNotFullyUploaded {
                missing_count: pending as u32,
            }));
        }

        let all_files = self.storage().buck_storage().get_all_files(cl_link).await?;
        for file in &all_files {
            if file.blob_id.is_none() {
                return Err(MegaError::Buck(BuckError::ValidationError(format!(
                    "Missing blob_id for file: {} (status: {})",
                    file.file_path, file.upload_status
                ))));
            }
        }

        // Build commit
        let file_changes: Vec<FileChange> = all_files
            .iter()
            .filter(|f| f.upload_status == upload_status::UPLOADED)
            .map(|f| {
                let blob_id = f.blob_id.as_ref().unwrap();
                let normalized_blob_id =
                    format!("sha1:{}", blob_id.strip_prefix("sha1:").unwrap_or(blob_id));
                FileChange::new(
                    f.file_path.clone(),
                    normalized_blob_id,
                    f.file_mode
                        .clone()
                        .unwrap_or_else(|| DEFAULT_MODE.to_string()),
                )
            })
            .collect();

        // Use commit_message from session
        let commit_message = session
            .commit_message
            .clone()
            .unwrap_or_else(|| "Upload via buck push".to_string());

        let commit_result = if file_changes.is_empty() {
            None
        } else {
            let builder = BuckCommitBuilder::new(self.storage().mono_storage());
            let result = builder
                .build_commit(
                    session.from_hash.as_deref().unwrap_or_default(),
                    &file_changes,
                    &commit_message,
                )
                .await?;
            Some(result)
        };

        // Convert to artifacts acceptable by BuckService
        let artifacts = commit_result.map(|res| {
            let commit_model: callisto::mega_commit::ActiveModel = res
                .commit
                .clone()
                .into_mega_model(git_internal::internal::metadata::EntryMeta::default())
                .into();
            let new_tree_models: Vec<callisto::mega_tree::ActiveModel> =
                res.new_tree_models.into_iter().map(|m| m.into()).collect();
            CommitArtifacts {
                commit_id: res.commit_id,
                tree_hash: res.tree_hash,
                new_tree_models,
                commit_model,
            }
        });

        let svc_resp: SvcCompleteResponse = self
            .storage()
            .buck_service
            .complete_upload(username, cl_link, SvcCompletePayload {}, artifacts)
            .await?;

        // Calculate uploaded files count
        let uploaded_files_count = file_changes.len() as u32;

        let response = CompleteResponse {
            cl_id: session.id,
            cl_link: session.session_id.clone(),
            commit_id: svc_resp.commit_id,
            files_count: uploaded_files_count,
            created_at: session.created_at.to_string(),
            repo_path: session.repo_path.clone(),
            from_hash: session.from_hash.clone().unwrap_or_default(),
        };

        self.trigger_build_for_buck_upload(&response, username);

        Ok(response)
    }
}
