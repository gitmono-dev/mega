use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::extract::FromRef;
use ceres::{
    TransportRuntime,
    application::{
        api_service::{
            ApiHandler,
            import_api_service::ImportApiService,
            mono::{MonoApiService, MonoAppServices},
        },
        build_trigger::BuildDispatchPort,
    },
    transport::protocol::repo::Repo,
};
use common::errors::MegaError;
use jupiter::storage::{Storage, user_storage::UserStorage};
use orion_scheduler_client::OrionSchedulerClient;
use saturn::entitystore::EntityStore;
use tower_sessions::MemoryStore;

use crate::api::oauth::api_store::OAuthApiStore;
pub mod api_common;
pub mod api_doc;
pub mod api_router;
pub mod error;
pub mod guard;
pub mod notes;
pub mod oauth;
pub mod router;

#[derive(Clone)]
pub struct MonoApiServiceState {
    services: MonoAppServices,
    session_store: Option<OAuthApiStore>,
    listen_addr: String,
    entity_store: EntityStore,
    orion_scheduler_client: Option<Arc<OrionSchedulerClient>>,
}

impl MonoApiServiceState {
    pub fn new(
        storage: Storage,
        git_object_cache: Arc<ceres::application::api_service::cache::GitObjectCache>,
        build_dispatch: Arc<dyn BuildDispatchPort>,
        session_store: Option<OAuthApiStore>,
        listen_addr: String,
        entity_store: EntityStore,
        orion_scheduler_client: Option<Arc<OrionSchedulerClient>>,
    ) -> Self {
        Self {
            services: MonoAppServices::new(storage, git_object_cache, Some(build_dispatch)),
            session_store,
            listen_addr,
            entity_store,
            orion_scheduler_client,
        }
    }

    pub fn orion_scheduler_client(&self) -> Option<&OrionSchedulerClient> {
        self.orion_scheduler_client
            .as_ref()
            .filter(|c| c.is_configured())
            .map(|c| c.as_ref())
    }

    pub fn listen_addr(&self) -> &str {
        &self.listen_addr
    }

    pub fn git(&self) -> &MonoApiService {
        self.services.git()
    }

    pub fn services(&self) -> &MonoAppServices {
        &self.services
    }

    pub(crate) async fn api_handler(&self, path: &Path) -> Result<Box<dyn ApiHandler>, MegaError> {
        let path = if path.has_root() {
            path.to_path_buf()
        } else {
            PathBuf::from("/").join(path)
        };

        let path_str = path
            .to_str()
            .ok_or_else(|| MegaError::bad_request("Invalid repository path"))?;

        let monorepo = self.git();
        let import_dir = monorepo.import_dir();
        if path.starts_with(&import_dir)
            && path != import_dir
            && let Some(model) = monorepo.find_git_repo_like_path(path_str).await?
        {
            let repo: Repo = model.into();
            return Ok(Box::new(ImportApiService::new(
                monorepo.storage().clone(),
                repo,
                monorepo.git_object_cache(),
            )));
        }

        Ok(Box::new(monorepo.clone()) as Box<dyn ApiHandler>)
    }
}

impl FromRef<MonoApiServiceState> for MemoryStore {
    fn from_ref(_: &MonoApiServiceState) -> Self {
        MemoryStore::default()
    }
}

impl FromRef<MonoApiServiceState> for OAuthApiStore {
    fn from_ref(state: &MonoApiServiceState) -> Self {
        state.session_store.clone().unwrap()
    }
}

impl FromRef<MonoApiServiceState> for UserStorage {
    fn from_ref(state: &MonoApiServiceState) -> Self {
        state.services.storage().user_storage()
    }
}

impl FromRef<MonoApiServiceState> for EntityStore {
    fn from_ref(state: &MonoApiServiceState) -> Self {
        state.entity_store.clone()
    }
}

impl FromRef<MonoApiServiceState> for TransportRuntime {
    fn from_ref(state: &MonoApiServiceState) -> TransportRuntime {
        state.services.transport_runtime().clone()
    }
}
