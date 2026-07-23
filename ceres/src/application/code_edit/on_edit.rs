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
                .create_new_cl(storage, repo_path, &self.from_hash, to_hash, username, None)
                .await?),
            EditCLMode::TryReuse(None) => Ok(editor
                .update_or_create_cl(storage, &self.from_hash, to_hash, username, None)
                .await?),
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
                    editor
                        .sync_cl_ref(storage, &existing_cl, to_hash)
                        .await
                        .map_err(|e| GitError::CustomError(e.to_string()))?;
                    let fresh = storage
                        .cl_storage()
                        .get_cl(&link)
                        .await
                        .map_err(|e| GitError::CustomError(e.to_string()))?;
                    Ok(fresh.unwrap_or(existing_cl))
                }
                _ => Err(GitError::CustomError(format!("link {} not found", link))),
            },
        }
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
    async fn onedit_visitor_creates_and_updates_cl_ref() {
        let dir = TempDir::new().unwrap();
        let storage = jupiter::tests::test_storage(dir.path()).await;
        let visitor = OneditVisitor {
            mono_storage: storage.mono_storage(),
        };
        let cl = sample_cl("ONEDIT01", "/project/demo");

        let created = visitor
            .visit(&cl, &"a".repeat(40), &"b".repeat(40))
            .await
            .expect("create");
        assert_eq!(created.ref_name, utils::cl_ref_name(&cl.link));

        let updated = visitor
            .visit(&cl, &"c".repeat(40), &"d".repeat(40))
            .await
            .expect("update without duplicate insert error");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.ref_commit_hash, "c".repeat(40));
    }

    #[tokio::test]
    async fn onedit_acceptor_writes_cl_ref() {
        let dir = TempDir::new().unwrap();
        let storage = jupiter::tests::test_storage(dir.path()).await;
        let visitor = OneditVisitor {
            mono_storage: storage.mono_storage(),
        };
        let cl = sample_cl("ONEDIT02", "/project/demo");

        OneditAcceptor {}
            .accept(&visitor, &cl, &"1".repeat(40), &"2".repeat(40))
            .await
            .unwrap();

        let saved = storage
            .mono_storage()
            .get_ref_by_name(&utils::cl_ref_name(&cl.link))
            .await
            .unwrap()
            .expect("ref present");
        assert!(saved.is_cl);
    }
}
