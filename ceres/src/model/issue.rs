use std::collections::HashSet;

use callisto::{mega_cl, mega_issue, sea_orm_active_enums::MergeStatusEnum};
use chrono::NaiveDateTime;
use jupiter::model::{
    common::{ItemDetails, ItemKind},
    issue_dto::IssueDetails,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::model::{conversation::ConversationItem, label::LabelItem};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ItemRes {
    pub id: i64,
    pub link: String,
    pub title: String,
    pub status: String,
    pub author: String,
    /// True when `author` matches a registered bot.
    #[serde(default)]
    pub author_is_bot: bool,
    pub open_timestamp: i64,
    pub closed_at: Option<i64>,
    pub merge_timestamp: Option<i64>,
    pub updated_at: i64,
    pub labels: Vec<LabelItem>,
    pub assignees: Vec<String>,
    pub comment_num: usize,
    /// Aggregated Orion build status for this CL (latest task), if any.
    /// Issue list leaves this unset/`null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_status: Option<String>,
}

impl From<ItemDetails> for ItemRes {
    fn from(value: ItemDetails) -> Self {
        match value.item {
            ItemKind::Issue(model) => Self {
                id: model.id,
                link: model.link,
                title: model.title,
                status: model.status.to_string(),
                author: model.author,
                author_is_bot: false,
                open_timestamp: model.created_at.and_utc().timestamp(),
                merge_timestamp: None,
                closed_at: model.closed_at.map(|dt| dt.and_utc().timestamp()),
                updated_at: model.updated_at.and_utc().timestamp(),
                labels: value.labels.into_iter().map(|m| m.into()).collect(),
                assignees: value.assignees,
                comment_num: value.comment_num,
                build_status: None,
            },
            ItemKind::Cl(model) => Self {
                id: model.id,
                link: model.link,
                title: model.title,
                status: format!("{:?}", model.status),
                author: model.username,
                author_is_bot: false,
                open_timestamp: model.created_at.and_utc().timestamp(),
                merge_timestamp: model.merge_date.map(|dt| dt.and_utc().timestamp()),
                closed_at: None,
                updated_at: model.updated_at.and_utc().timestamp(),
                labels: value.labels.into_iter().map(|m| m.into()).collect(),
                assignees: value.assignees,
                comment_num: value.comment_num,
                build_status: None,
            },
        }
    }
}

impl ItemRes {
    pub fn apply_bot_flags(items: &mut [Self], bot_names: &HashSet<String>) {
        for item in items {
            item.author_is_bot = bot_names.contains(&item.author) || item.author == "system";
        }
    }

    pub fn apply_build_statuses(
        items: &mut [Self],
        statuses: &std::collections::HashMap<String, String>,
    ) {
        for item in items {
            item.build_status = statuses.get(&item.link).cloned();
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct NewIssue {
    pub title: String,
    pub description: String,
}

#[derive(Serialize, ToSchema)]
pub struct IssueDetailRes {
    pub id: i64,
    pub link: String,
    pub title: String,
    pub status: String,
    pub author: String,
    #[serde(default)]
    pub author_is_bot: bool,
    pub open_timestamp: i64,
    pub conversations: Vec<ConversationItem>,
    pub labels: Vec<LabelItem>,
    pub assignees: Vec<String>,
}

impl From<IssueDetails> for IssueDetailRes {
    fn from(value: IssueDetails) -> Self {
        Self {
            id: value.issue.id,
            link: value.issue.link,
            title: value.issue.title,
            status: value.issue.status.to_string(),
            author: value.issue.author.clone(),
            author_is_bot: false,
            open_timestamp: value.issue.created_at.and_utc().timestamp(),
            conversations: value
                .conversations
                .into_iter()
                .map(|x| ConversationItem::from_model(x.conversation, x.reactions, &value.username))
                .collect(),
            labels: value.labels.into_iter().map(|x| x.into()).collect(),
            assignees: value
                .assignees
                .into_iter()
                .map(|x| x.assignnee_id)
                .collect(),
        }
    }
}

#[derive(Serialize, ToSchema, PartialEq, Eq)]
pub struct IssueSuggestions {
    pub id: i64,
    pub link: String,
    pub title: String,
    #[serde(rename = "type")]
    pub suggest_type: String,
    #[serde(skip)]
    pub created_at: NaiveDateTime,
}

impl PartialOrd for IssueSuggestions {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IssueSuggestions {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.created_at.cmp(&other.created_at)
    }
}

impl From<mega_issue::Model> for IssueSuggestions {
    fn from(value: mega_issue::Model) -> Self {
        Self {
            id: value.id,
            link: value.link,
            title: value.title,
            suggest_type: if value.status == "open" {
                String::from("issue_open")
            } else {
                String::from("issue_closed")
            },
            created_at: value.created_at,
        }
    }
}

impl From<mega_cl::Model> for IssueSuggestions {
    fn from(value: mega_cl::Model) -> Self {
        Self {
            id: value.id,
            link: value.link,
            title: value.title,
            suggest_type: if value.status == MergeStatusEnum::Open {
                String::from("change_list")
            } else {
                String::from("change_list_closed")
            },
            created_at: value.created_at,
        }
    }
}

#[derive(Deserialize, ToSchema, IntoParams)]
pub struct QueryPayload {
    pub query: String,
}
