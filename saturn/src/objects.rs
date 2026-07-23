use std::collections::HashSet;

use cedar_policy::{Entity, RestrictedExpression};
use serde::{Deserialize, Serialize};

use crate::util::SaturnEUid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    euid: SaturnEUid,
    parents: HashSet<SaturnEUid>,
}

impl User {
    /// Get the entity unique identifier.
    pub fn euid(&self) -> &SaturnEUid {
        &self.euid
    }

    /// Get the parent groups this user belongs to.
    pub fn parents(&self) -> &HashSet<SaturnEUid> {
        &self.parents
    }
}

impl From<User> for Entity {
    fn from(value: User) -> Entity {
        Entity::new_no_attrs(
            value.euid.into(),
            value.parents.into_iter().map(|euid| euid.into()).collect(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserGroup {
    euid: SaturnEUid,
    parents: HashSet<SaturnEUid>,
}

impl From<UserGroup> for Entity {
    fn from(value: UserGroup) -> Entity {
        Entity::new_no_attrs(
            value.euid.into(),
            value.parents.into_iter().map(|euid| euid.into()).collect(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    euid: SaturnEUid,
    is_private: bool,
    admins: SaturnEUid,
    maintainers: SaturnEUid,
    readers: SaturnEUid,
    parents: HashSet<SaturnEUid>,
}

impl From<Repo> for Entity {
    fn from(value: Repo) -> Self {
        let attrs = [
            (
                "is_private",
                RestrictedExpression::new_bool(value.is_private),
            ),
            (
                "admins",
                format!("{}", value.admins.as_ref()).parse().unwrap(),
            ),
            (
                "maintainers",
                format!("{}", value.maintainers.as_ref()).parse().unwrap(),
            ),
            (
                "readers",
                format!("{}", value.readers.as_ref()).parse().unwrap(),
            ),
        ]
        .into_iter()
        .map(|(x, v)| (x.into(), v))
        .collect();

        let parents = value.parents.into_iter().map(|euid| euid.into()).collect();

        Entity::new(value.euid.into(), attrs, parents).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeList {
    euid: SaturnEUid,
    repo: SaturnEUid,
    parents: HashSet<SaturnEUid>,
}

impl From<ChangeList> for Entity {
    fn from(value: ChangeList) -> Entity {
        let attrs = [("repo", format!("{}", value.repo.as_ref()).parse().unwrap())]
            .into_iter()
            .map(|(x, v)| (x.into(), v))
            .collect();

        Entity::new(
            value.euid.into(),
            attrs,
            value.parents.into_iter().map(|euid| euid.into()).collect(),
        )
        .unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    euid: SaturnEUid,
    repo: SaturnEUid,
    parents: HashSet<SaturnEUid>,
}

impl From<Issue> for Entity {
    fn from(value: Issue) -> Entity {
        let attrs = [("repo", format!("{}", value.repo.as_ref()).parse().unwrap())]
            .into_iter()
            .map(|(x, v)| (x.into(), v))
            .collect();

        Entity::new(
            value.euid.into(),
            attrs,
            value.parents.into_iter().map(|euid| euid.into()).collect(),
        )
        .unwrap()
    }
}
