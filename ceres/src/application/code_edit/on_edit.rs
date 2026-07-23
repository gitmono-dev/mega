use callisto::{mega_cl, mega_refs};
use common::{
    errors::MegaError,
    utils::{self},
};
use git_internal::errors::GitError;
use jupiter::storage::{Storage, mono_storage::MonoStorage};

use crate::{
    application::{
        api_service::{cache::GitObjectCache, mono::MonoApiService},
        build_trigger::{BuildTriggerService, TriggerContext},
        code_edit::{model, utils as edit_utils},
    },
    model::git::EditCLMode,
};

pub struct OneditFormator;
impl model::ConversationMessageFormater for OneditFormator {
    fn format(&self, _: &mega_cl::Model, from_hash: &str, to_hash: &str, username: &str) -> String {
        let old_hash = &from_hash[..6];
        let new_hash = &to_hash[..6];
        format!(
            "{} edited the change_list automatic from {} to {}.",
            username, old_hash, new_hash
        )
    }
}

pub struct OneditVisitor {
    mono_storage: MonoStorage,
}
impl model::CLRefUpdateVisitor for OneditVisitor {
    async fn visit(
        &self,
        cl: &mega_cl::Model,
        commit_hash: &str,
        tree_hash: &str,
    ) -> Result<mega_refs::Model, MegaError> {
        let cl_ref = mega_refs::Model::new(
            &cl.path,
            utils::cl_ref_name(&cl.link),
            commit_hash.to_string(),
            tree_hash.to_string(),
            true,
        );
        self.mono_storage.save_refs(cl_ref.clone(), None).await?;
        Ok(cl_ref)
    }
}

pub struct OneditAcceptor {}

impl<VT: model::CLRefUpdateVisitor> model::CLRefUpdateAcceptor<VT> for OneditAcceptor {
    async fn accept(
        &self,
        visitor: &VT,
        cl: &mega_cl::Model,
        commit_hash: &str,
        tree_hash: &str,
    ) -> Result<(), MegaError> {
        visitor.visit(cl, commit_hash, tree_hash).await?;
        Ok(())
    }
}

pub struct OneditTrigerBuilder {}

impl model::TriggerContextBuilder for OneditTrigerBuilder {
    async fn get_context(
        &self,
        cl: &mega_cl::Model,
        username: &str,
    ) -> Result<TriggerContext, MegaError> {
        Ok(TriggerContext::from_git_push(
            cl.path.clone(),
            cl.from_hash.clone(),
            cl.to_hash.clone(),
            cl.link.clone(),
            Some(cl.id),
            Some(cl.path.clone()),
            Some(username.to_string()),
        ))
    }

    async fn trigger_build(
        &self,
        storage: Storage,
        git_cache: std::sync::Arc<GitObjectCache>,
        build_dispatch: std::sync::Arc<dyn crate::application::build_trigger::BuildDispatchPort>,
        cl: &mega_cl::Model,
        username: &str,
    ) -> Result<(), MegaError> {
        let cl_model = cl.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let repo_path =
                match edit_utils::resolve_build_repo_root(&storage, &cl_model.path).await {
                    Ok(repo_path) => repo_path,
                    Err(e) => {
                        tracing::error!(
                            cl_link = %cl_model.link,
                            cl_path = %cl_model.path,
                            "Failed to resolve build repo root for web edit: {}",
                            e
                        );
                        return Err(e);
                    }
                };
            let context = TriggerContext::from_git_push(
                repo_path,
                cl_model.from_hash.clone(),
                cl_model.to_hash.clone(),
                cl_model.link.clone(),
                Some(cl_model.id),
                Some(cl_model.path.clone()),
                Some(username),
            );
            BuildTriggerService::build_by_context(storage, git_cache, build_dispatch, context).await
        });
        Ok(())
    }
}

pub struct OneditChecker {}

impl model::Checker for OneditChecker {}

pub(crate) type OneditCodeEdit = model::CodeEditService<
    OneditFormator,
    OneditVisitor,
    OneditAcceptor,
    OneditTrigerBuilder,
    OneditChecker,
    MonoApiService,
    model::DefualtDirector<MonoApiService>,
>;

impl OneditCodeEdit {
    pub fn from(
        repo_path: &str,
        base_branch: &str,
        from_hash: &str,
        handler: &MonoApiService,
        mono_storage: MonoStorage,
    ) -> Self {
        Self::new(
            repo_path,
            base_branch,
            from_hash,
            OneditFormator {},
            OneditVisitor { mono_storage },
            OneditAcceptor {},
            OneditTrigerBuilder {},
            OneditChecker {},
            model::DefualtDirector::<MonoApiService> {
                handler: handler.clone(),
            },
        )
    }

    pub async fn find_or_create_cl_for_edit(
        &self,
        storage: &Storage,
        editor: &OneditCodeEdit,
        mode: EditCLMode,
        to_hash: &str,
        username: &str,
    ) -> Result<mega_cl::Model, GitError> {
        let repo_path = &self.repo_path;
        match mode {
            EditCLMode::ForceCreate => Ok(editor
                .create_new_cl(storage, repo_path, &self.from_hash, to_hash, username)
                .await?),
            EditCLMode::TryReuse(None) => {
                if let Some(existing_cl) = storage
                    .cl_storage()
                    .get_open_cl_by_path(repo_path, username)
                    .await
                    .map_err(|e| GitError::CustomError(format!("Failed to fetch CL: {}", e)))?
                {
                    editor
                        .update_existing_cl(
                            existing_cl.clone(),
                            storage,
                            &existing_cl.from_hash,
                            to_hash,
                            username,
                        )
                        .await?;
                    Ok(existing_cl)
                } else {
                    Ok(editor
                        .create_new_cl(storage, repo_path, &self.from_hash, to_hash, username)
                        .await?)
                }
            }
            EditCLMode::TryReuse(Some(link)) => match storage.cl_storage().get_cl(&link).await {
                Ok(Some(existing_cl)) => {
                    editor
                        .update_existing_cl(
                            existing_cl.clone(),
                            storage,
                            &existing_cl.from_hash,
                            to_hash,
                            username,
                        )
                        .await?;
                    Ok(existing_cl)
                }
                _ => Err(GitError::CustomError(format!("link {} not found", link))),
            },
        }
    }
}
