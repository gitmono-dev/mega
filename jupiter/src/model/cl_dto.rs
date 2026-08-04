use callisto::{item_assignees, label, mega_cl, sea_orm_active_enums::MergeStatusEnum};
use sea_orm::entity::prelude::*;

use crate::model::conv_dto::ConvWithReactions;

pub struct CLDetails {
    pub username: String,
    pub cl: mega_cl::Model,
    pub conversations: Vec<ConvWithReactions>,
    pub labels: Vec<label::Model>,
    pub assignees: Vec<item_assignees::Model>,
}

pub struct ClInfoDto {
    pub link: String,
    pub title: String,
    pub merge_date: Option<DateTime>,
    pub status: MergeStatusEnum,
    pub path: String,
    pub from_hash: String,
    pub to_hash: String,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub username: String,
}

impl From<mega_cl::Model> for ClInfoDto {
    fn from(value: mega_cl::Model) -> Self {
        Self {
            link: value.link,
            title: value.title,
            merge_date: value.merge_date,
            status: value.status,
            path: value.path,
            from_hash: value.from_hash,
            to_hash: value.to_hash,
            created_at: value.created_at,
            updated_at: value.updated_at,
            username: value.campsite_user_id,
        }
    }
}
