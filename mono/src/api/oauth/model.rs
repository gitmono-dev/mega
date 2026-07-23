use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CampsiteUserJson {
    pub username: String,
    pub id: String,
    pub avatar_url: String,
    pub email: Option<String>,
    #[serde(default)]
    pub github_login: Option<String>,
}

impl From<CampsiteUserJson> for LoginUser {
    fn from(value: CampsiteUserJson) -> Self {
        Self {
            username: value.username,
            email: value.email.unwrap_or_default(),
            avatar_url: value.avatar_url,
            campsite_user_id: value.id,
            github_login: value.github_login.filter(|s| !s.trim().is_empty()),
        }
    }
}

/// Tinyship / better-auth `GET /api/auth/get-session` response body.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TinyshipGetSessionResponse {
    pub session: Option<TinyshipSessionJson>,
    pub user: Option<TinyshipAuthUserJson>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TinyshipSessionJson {
    pub id: String,
    pub user_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TinyshipAuthUserJson {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub image: Option<String>,
}

impl From<TinyshipAuthUserJson> for LoginUser {
    fn from(value: TinyshipAuthUserJson) -> Self {
        Self {
            campsite_user_id: value.id,
            username: value.name,
            email: value.email.unwrap_or_default(),
            avatar_url: value.image.unwrap_or_default(),
            github_login: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LoginUser {
    pub campsite_user_id: String,
    pub username: String,
    /// GitHub login when Campsite authenticated via GitHub; used as Cedar User euid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    pub avatar_url: String,
    pub email: String,
}

impl LoginUser {
    /// Identity for Cedar / `.mega_cedar.json` admin checks: GitHub login when present.
    pub fn cedar_user_id(&self) -> &str {
        self.github_login
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(self.username.as_str())
    }
}
