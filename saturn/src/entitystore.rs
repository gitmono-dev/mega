use std::collections::{HashMap, HashSet};

use cedar_policy::{Entities, Schema};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_string_pretty};

use crate::{
    objects::{ChangeList, Issue, Repo, User, UserGroup},
    util::SaturnEUid,
};

/// An in-memory store for entities used in Cedar policies.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct EntityStore {
    users: HashMap<SaturnEUid, User>,
    repos: HashMap<SaturnEUid, Repo>,
    #[serde(alias = "merge_requests")]
    change_lists: HashMap<SaturnEUid, ChangeList>,
    issues: HashMap<SaturnEUid, Issue>,
    user_groups: HashMap<SaturnEUid, UserGroup>,
}

impl EntityStore {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            repos: HashMap::new(),
            change_lists: HashMap::new(),
            issues: HashMap::new(),
            user_groups: HashMap::new(),
        }
    }

    pub fn as_entities(&self, schema: &Schema) -> Entities {
        let users = self.users.values().map(|user| user.clone().into());
        let repos = self.repos.values().map(|repo| repo.clone().into());
        let change_lists = self.change_lists.values().map(|cl| cl.clone().into());
        let issues = self.issues.values().map(|repo| repo.clone().into());
        let user_groups = self.user_groups.values().map(|group| group.clone().into());
        let all = users
            .chain(repos)
            .chain(user_groups)
            .chain(change_lists)
            .chain(issues);
        Entities::from_entities(all, Some(schema)).unwrap()
    }

    pub fn merge(&mut self, other: EntityStore) {
        self.users.extend(other.users);
        self.repos.extend(other.repos);
        self.change_lists.extend(other.change_lists);
        self.issues.extend(other.issues);
        self.user_groups.extend(other.user_groups);
    }

    pub fn extract_admin_usernames(&self) -> HashSet<String> {
        const ADMIN_GROUP: &str = "UserGroup::\"admin\"";

        let mut admins = HashSet::new();
        for user in self.users.values() {
            let is_admin = user.parents().iter().any(|p| p.to_string() == ADMIN_GROUP);
            if is_admin {
                let username: &str = user.euid().id().as_ref();
                admins.insert(username.to_string());
            }
        }
        admins
    }
}

pub fn generate_entity(
    admins: &[String],
    repo: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut json_data = json!({
        "users": {
        },
        "repos": {
        },
        "user_groups": {
            "UserGroup::\"admin\"": {
                "euid": "UserGroup::\"admin\"",
                "parents": [
                    "UserGroup::\"matainer\""
                ]
            },
            "UserGroup::\"matainer\"": {
                "euid": "UserGroup::\"matainer\"",
                "parents": [
                    "UserGroup::\"reader\""
                ]
            },
            "UserGroup::\"reader\"": {
                "euid": "UserGroup::\"reader\"",
                "parents": []
            }
        },
        "change_lists": {
        },
        "issues": {
        }
    });

    // Add all admin users to the admin group
    if let Some(users) = json_data.get_mut("users")
        && let Some(users_map) = users.as_object_mut()
    {
        for user in admins {
            users_map.insert(
                format!("User::\"{user}\""),
                json!({
                        "euid": format!("User::\"{user}\""),
                        "parents": [
                            "UserGroup::\"admin\""
                        ]
                }),
            );
        }
    }

    if let Some(repos) = json_data.get_mut("repos")
        && let Some(repos_map) = repos.as_object_mut()
    {
        repos_map.insert(
            format!("Repository::\"{repo}\""),
            json!({
                    "euid": format!("Repository::\"{repo}\""),
                    "is_private": true,
                    "admins": "UserGroup::\"admin\"",
                    "maintainers": "UserGroup::\"matainer\"",
                    "readers": "UserGroup::\"reader\"",
                    "parents": []
            }),
        );
    }
    Ok(to_string_pretty(&json_data)?)
}
