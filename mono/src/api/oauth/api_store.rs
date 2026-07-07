use async_trait::async_trait;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
    session_store::Result,
};

use crate::api::oauth::{
    campsite_store::CampsiteApiStore, model::LoginUser, tinyship_store::TinyshipApiStore,
};

#[derive(Debug, Clone)]
pub enum OAuthApiStore {
    Campsite(CampsiteApiStore),
    Tinyship(TinyshipApiStore),
}

impl OAuthApiStore {
    pub fn session_cookie_name(&self) -> &str {
        match self {
            OAuthApiStore::Campsite(store) => store.session_cookie_name(),
            OAuthApiStore::Tinyship(store) => store.session_cookie_name(),
        }
    }

    pub async fn load_user_from_api(
        &self,
        cookie_value: String,
    ) -> anyhow::Result<Option<LoginUser>> {
        match self {
            OAuthApiStore::Campsite(store) => store.load_user_from_api(cookie_value).await,
            OAuthApiStore::Tinyship(store) => store.load_user_from_api(cookie_value).await,
        }
    }
}

#[async_trait]
impl SessionStore for OAuthApiStore {
    async fn save(&self, record: &Record) -> Result<()> {
        match self {
            OAuthApiStore::Campsite(store) => store.save(record).await,
            OAuthApiStore::Tinyship(store) => store.save(record).await,
        }
    }

    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        match self {
            OAuthApiStore::Campsite(store) => store.load(session_id).await,
            OAuthApiStore::Tinyship(store) => store.load(session_id).await,
        }
    }

    async fn delete(&self, session_id: &Id) -> Result<()> {
        match self {
            OAuthApiStore::Campsite(store) => store.delete(session_id).await,
            OAuthApiStore::Tinyship(store) => store.delete(session_id).await,
        }
    }
}
