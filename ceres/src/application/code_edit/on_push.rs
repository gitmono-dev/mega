use std::sync::Arc;

use callisto::{mega_cl, mega_refs};
use common::{errors::MegaError, utils};
use jupiter::storage::{Storage, mono_storage::MonoStorage};

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

pub struct OnpushVisitor {
    mono_storage: MonoStorage,
}

impl model::CLRefUpdateVisitor for OnpushVisitor {
    async fn visit(
        &self,
        cl: &mega_cl::Model,
        commit_hash: &str,
        tree_hash: &str,
    ) -> Result<mega_refs::Model, MegaError> {
        // Pack receive usually writes refs/cl/{link} first; save_or_update keeps create and
        // update paths aligned when that step was skipped (e.g. empty pack / no current_commit).
        let ref_name = utils::cl_ref_name(&cl.link);
        self.mono_storage
            .save_or_update_cl_ref(&cl.path, &ref_name, commit_hash, tree_hash)
            .await?;
        self.mono_storage
            .get_ref_by_name(&ref_name)
            .await?
            .ok_or_else(|| MegaError::Other(format!("CL ref missing after save: {ref_name}")))
    }
}

pub struct OnpushAcceptor {}

impl<VT: CLRefUpdateVisitor> model::CLRefUpdateAcceptor<VT> for OnpushAcceptor {
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
            OnpushVisitor {
                mono_storage: handler.storage().mono_storage(),
            },
            OnpushAcceptor {},
            OnpushTrigerBuilder {},
            OnpushChecker {},
            model::DefualtDirector {
                handler: handler.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use callisto::sea_orm_active_enums::MergeStatusEnum;
    use common::utils;
    use tempfile::TempDir;

    use super::*;
    use crate::application::code_edit::model::{CLRefUpdateAcceptor, CLRefUpdateVisitor};

    fn sample_cl(link: &str, path: &str) -> mega_cl::Model {
        let now = chrono::Utc::now().naive_utc();
        mega_cl::Model {
            id: 1,
            link: link.to_string(),
            title: "test".to_string(),
            merge_date: None,
            status: MergeStatusEnum::Open,
            path: path.to_string(),
            from_hash: "1".repeat(40),
            to_hash: "2".repeat(40),
            created_at: now,
            updated_at: now,
            username: "tester".to_string(),
            base_branch: "main".to_string(),
        }
    }

    #[tokio::test]
    async fn onpush_visitor_creates_and_updates_cl_ref() {
        let dir = TempDir::new().unwrap();
        let storage = jupiter::tests::test_storage(dir.path()).await;
        let visitor = OnpushVisitor {
            mono_storage: storage.mono_storage(),
        };
        let cl = sample_cl("ONPUSH01", "/toolchains");

        let created = visitor
            .visit(&cl, &"a".repeat(40), &"b".repeat(40))
            .await
            .expect("create");
        assert_eq!(created.ref_name, utils::cl_ref_name(&cl.link));
        assert_eq!(created.ref_commit_hash, "a".repeat(40));

        let updated = visitor
            .visit(&cl, &"c".repeat(40), &"d".repeat(40))
            .await
            .expect("update");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.ref_commit_hash, "c".repeat(40));
        assert_eq!(updated.ref_tree_hash, "d".repeat(40));
    }

    #[tokio::test]
    async fn onpush_acceptor_delegates_to_visitor() {
        let dir = TempDir::new().unwrap();
        let storage = jupiter::tests::test_storage(dir.path()).await;
        let visitor = OnpushVisitor {
            mono_storage: storage.mono_storage(),
        };
        let cl = sample_cl("ONPUSH02", "/project/demo");

        OnpushAcceptor {}
            .accept(&visitor, &cl, &"1".repeat(40), &"2".repeat(40))
            .await
            .unwrap();

        let saved = storage
            .mono_storage()
            .get_ref_by_name(&utils::cl_ref_name(&cl.link))
            .await
            .unwrap()
            .expect("ref written by acceptor");
        assert!(saved.is_cl);
        assert_eq!(saved.path, cl.path);
    }
}
