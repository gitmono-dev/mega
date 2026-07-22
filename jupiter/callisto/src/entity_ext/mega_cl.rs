use sea_orm::entity::prelude::*;

use crate::{
    entity_ext::generate_id,
    mega_cl::{self, Entity},
    sea_orm_active_enums::MergeStatusEnum,
};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    ItemLabels,
    ItemAssignees,
    Conversation,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::ItemLabels => Entity::has_many(crate::item_labels::Entity).into(),
            Self::ItemAssignees => Entity::has_many(crate::item_assignees::Entity).into(),
            Self::Conversation => Entity::has_many(crate::mega_conversation::Entity).into(),
        }
    }
}

impl Related<crate::label::Entity> for Entity {
    fn to() -> RelationDef {
        crate::entity_ext::item_labels::Relation::Label.def()
    }

    fn via() -> Option<RelationDef> {
        Some(crate::entity_ext::item_labels::Relation::MegaCl.def().rev())
    }
}

impl Related<crate::item_assignees::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ItemAssignees.def()
    }
    fn via() -> Option<RelationDef> {
        None
    }
}

impl Related<crate::mega_conversation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Conversation.def()
    }
    fn via() -> Option<RelationDef> {
        None
    }
}

impl mega_cl::Model {
    pub fn new(
        path: String,
        title: String,
        link: String,
        base_branch: String,
        from_hash: String,
        to_hash: String,
        username: String,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: generate_id(),
            link,
            title: title.to_owned(),
            status: MergeStatusEnum::Open,
            created_at: now,
            updated_at: now,
            merge_date: None,
            path,
            base_branch,
            from_hash,
            to_hash,
            username,
        }
    }

    /// Create a new CL with Draft status
    pub fn new_draft(
        path: String,
        title: String,
        link: String,
        base_branch: String,
        from_hash: String,
        username: String,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: generate_id(),
            link,
            title: title.to_owned(),
            status: MergeStatusEnum::Draft,
            created_at: now,
            updated_at: now,
            merge_date: None,
            path,
            base_branch,
            from_hash,
            to_hash: String::new(),
            username,
        }
    }
}
