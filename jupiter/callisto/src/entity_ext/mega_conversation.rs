use sea_orm::entity::prelude::*;

use crate::{
    entity_ext::generate_id,
    mega_conversation::{self, Column, Entity},
    sea_orm_active_enums::ConvTypeEnum,
};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    MegaIssue,
    MegaCl,
    Reactions,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::MegaIssue => Entity::belongs_to(crate::mega_issue::Entity)
                .from(Column::Link)
                .to(crate::mega_issue::Column::Link)
                .into(),
            Self::MegaCl => Entity::belongs_to(crate::mega_cl::Entity)
                .from(Column::Link)
                .to(crate::mega_cl::Column::Link)
                .into(),
            Self::Reactions => Entity::has_many(crate::reactions::Entity).into(),
        }
    }
}

impl Related<crate::mega_issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MegaIssue.def()
    }
}

impl Related<crate::mega_cl::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MegaCl.def()
    }
}

impl Related<crate::reactions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Reactions.def()
    }
    fn via() -> Option<RelationDef> {
        None
    }
}

impl mega_conversation::Model {
    pub fn new(
        link: &str,
        conv_type: ConvTypeEnum,
        comment: Option<String>,
        username: &str,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();
        let resolved = if conv_type == ConvTypeEnum::Review {
            Some(false)
        } else {
            None
        };

        Self {
            id: generate_id(),
            link: link.to_owned(),
            conv_type,
            comment,
            created_at: now,
            updated_at: now,
            username: username.to_owned(),
            resolved,
        }
    }
}
