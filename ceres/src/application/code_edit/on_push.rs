use std::sync::Arc;

use callisto::{mega_cl, mega_refs};
use common::errors::MegaError;
use jupiter::storage::Storage;

use crate::application::{
    api_service::{cache::GitObjectCache, mono::MonoApiService},
    build_trigger::{BuildTriggerService, SharedBuildDispatch, TriggerContext},
    code_edit::{
        model::{self, CLRefUpdateVisitor},
        utils as edit_utils,
    },
};

pub struct OnpushFormator;
impl model::ConversationMessageFormater for OnpushFormator {}

pub struct OnpushVisitor {}
impl model::CLRefUpdateVisitor for OnpushVisitor {
    async fn visit(
        &self,
        _: &mega_cl::Model,
        _: &str,
        _: &str,
    ) -> Result<mega_refs::Model, MegaError> {
        Err(MegaError::Other(
            "OnpushVisitor::visit is unused; OnpushAcceptor does not delegate to the visitor"
                .to_string(),
        ))
    }
}

pub struct OnpushAcceptor {}

impl<VT: CLRefUpdateVisitor> model::CLRefUpdateAcceptor<VT> for OnpushAcceptor {
    async fn accept(&self, _: &VT, _: &mega_cl::Model, _: &str, _: &str) -> Result<(), MegaError> {
        Ok(())
    }
}

pub struct OnpushTrigerBuilder {}

impl model::TriggerContextBuilder for OnpushTrigerBuilder {
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
        git_cache: Arc<GitObjectCache>,
        build_dispatch: SharedBuildDispatch,
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
                            "Failed to resolve build repo root for git push: {}",
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

pub struct OnpushChecker {}

impl model::Checker for OnpushChecker {}

pub(crate) type OnpushCodeEdit = model::CodeEditService<
    OnpushFormator,
    OnpushVisitor,
    OnpushAcceptor,
    OnpushTrigerBuilder,
    OnpushChecker,
    MonoApiService,
    model::DefualtDirector<MonoApiService>,
>;

// impl<'a> model::CodeEditService<OnpushFormator, OnpushVisitor, OnpushAcceptor, OnpushTrigerBuilder, OnpushChecker, model::DefualthDirector<'a, MonoRepo>> {
impl OnpushCodeEdit {
    pub fn from(
        repo_path: &str,
        base_branch: &str,
        from_hash: &str,
        handler: &MonoApiService,
    ) -> Self {
        Self::new(
            repo_path,
            base_branch,
            from_hash,
            OnpushFormator {},
            OnpushVisitor {},
            OnpushAcceptor {},
            OnpushTrigerBuilder {},
            OnpushChecker {},
            model::DefualtDirector {
                handler: handler.clone(),
            },
        )
    }
}
