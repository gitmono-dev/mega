//! OAuth / session extractors for Axum API routes.
//!
//! Git smart HTTP (`/git-receive-pack`, etc.) is handled by [`crate::server::http_server::handle_smart_protocol`],
//! which takes a raw [`axum::http::Request`] and does not run `FromRequestParts`. For the same Mono access-token
//! validation as [`AccessTokenUser`], call [`bearer_token_from_authorization_value`] and
//! [`login_user_from_mono_access_token`] from that code path instead of the extractor.

use std::ops::Deref;

use axum::{
    RequestPartsExt,
    extract::{FromRef, FromRequestParts},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{self, Authorization, authorization::Bearer},
};
pub use ceres::model::bots::BotIdentity;
use common::errors::MegaError;
use http::request::Parts;
use jupiter::storage::user_storage::UserStorage;
use model::LoginUser;

use crate::api::{MonoApiServiceState, oauth::api_store::OAuthApiStore};

pub mod api_store;
pub mod campsite_store;
pub mod model;
pub mod tinyship_store;

pub struct AuthRedirect;

impl IntoResponse for AuthRedirect {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, "Login first").into_response()
    }
}
pub struct AccessTokenUser(pub LoginUser);

/// Authenticated user resolved from a **browser session cookie** (Campsite or Tinyship),
/// not from `Authorization: Bearer` or the Mono DB access-token table.
///
/// The Axum extractor reads the HTTP `Cookie` header, takes the value named by
/// [`OAuthApiStore::session_cookie_name`], and loads the user via [`OAuthApiStore::load_user_from_api`].
/// For API clients that send a Mono access token in `Authorization`, use [`AccessTokenUser`] instead.
pub struct SessionUser(pub LoginUser);

/// Parses a raw `Authorization` header value for `Bearer <token>` (case-insensitive `bearer` prefix).
/// Matches the Git HTTP receive-pack path so CLI clients and API routes share one rule.
pub fn bearer_token_from_authorization_value(value: &str) -> Option<&str> {
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
}

/// Validates a Mono DB access token; same as [`AccessTokenUser`] but usable outside Axum extractors.
pub async fn login_user_from_mono_access_token(
    user_storage: &UserStorage,
    token: &str,
) -> Result<Option<LoginUser>, MegaError> {
    let Some((campsite_user_id, github_login)) = user_storage.find_user_by_token(token).await?
    else {
        return Ok(None);
    };
    Ok(Some(LoginUser {
        campsite_user_id,
        github_login: github_login.filter(|s| !s.trim().is_empty()),
        ..Default::default()
    }))
}

/// Axum extractor for bot bearer tokens (`bot_` prefix).
pub struct BotAuth(pub BotIdentity);

impl Deref for BotAuth {
    type Target = BotIdentity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for BotAuth
where
    MonoApiServiceState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract Authorization: Bearer <token> header
        let bearer = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|e| {
                tracing::debug!("BotIdentity: missing or invalid Authorization header: {e}");
                AuthRedirect
            })?
            .0
            .0;

        let raw_token = bearer.token();
        const BOT_PREFIX: &str = "bot_";

        // Enforce bot_ prefix for bot identity routes
        if !raw_token.starts_with(BOT_PREFIX) {
            tracing::debug!("BotIdentity: bearer token does not start with expected bot_ prefix");
            return Err(AuthRedirect);
        }

        // Delegate token validation to Jupiter storage (BotsStorage)
        let state_ref = MonoApiServiceState::from_ref(state);

        match state_ref
            .services()
            .user()
            .find_bot_by_token(raw_token)
            .await
        {
            Ok(Some((bot, token))) => Ok(BotAuth(BotIdentity::from_models(bot, token))),
            Ok(None) => {
                tracing::warn!("BotIdentity: bot token not found, revoked, or expired");
                Err(AuthRedirect)
            }
            Err(e) => {
                tracing::error!("BotIdentity: error while validating bot token: {:?}", e);
                Err(AuthRedirect)
            }
        }
    }
}

impl<S> FromRequestParts<S> for AccessTokenUser
where
    UserStorage: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user_storage = UserStorage::from_ref(state);

        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|e| {
                tracing::debug!("AccessTokenUser: missing or invalid bearer token: {e}");
                AuthRedirect
            })?;

        match login_user_from_mono_access_token(&user_storage, bearer.token()).await {
            Ok(Some(user)) => Ok(AccessTokenUser(user)),
            Ok(None) => {
                tracing::debug!("AccessTokenUser: invalid or expired bearer token");
                Err(AuthRedirect)
            }
            Err(e) => {
                tracing::warn!("AccessTokenUser: error validating bearer token: {e:?}");
                Err(AuthRedirect)
            }
        }
    }
}

impl<S> FromRequestParts<S> for SessionUser
where
    OAuthApiStore: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    /// Reads the session cookie from the request and resolves [`LoginUser`] through the
    /// configured [`OAuthApiStore`] (external auth API). Missing cookie, unknown session, or
    /// API errors become [`AuthRedirect`] (401).
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let store = OAuthApiStore::from_ref(state);

        // Cookie: external auth session (Campsite or Tinyship per `OAuthApiStore`).
        let cookies = parts
            .extract::<TypedHeader<headers::Cookie>>()
            .await
            .map_err(|e| {
                tracing::debug!("SessionUser: failed to read Cookie header: {e}");
                AuthRedirect
            })?;

        let session_cookie = cookies
            .get(store.session_cookie_name())
            .ok_or(AuthRedirect)?;

        // Load user from external API
        match store.load_user_from_api(session_cookie.to_string()).await {
            Ok(Some(user)) => Ok(SessionUser(user)),
            Ok(None) => {
                tracing::debug!("SessionUser: invalid or expired session (external auth)");
                Err(AuthRedirect)
            }
            Err(e) => {
                tracing::warn!("SessionUser: error loading user from cookie session: {e:?}");
                Err(AuthRedirect)
            }
        }
    }
}

// Backward-compatible extractor: `LoginUser` now maps to cookie session only.
// Use `AccessTokenUser` explicitly where bearer token auth is required.
impl<S> FromRequestParts<S> for LoginUser
where
    OAuthApiStore: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let SessionUser(user) = SessionUser::from_request_parts(parts, state).await?;
        Ok(user)
    }
}
