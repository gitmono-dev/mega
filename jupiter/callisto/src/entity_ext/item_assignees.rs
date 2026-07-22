use sea_orm::entity::prelude::*;

use crate::item_assignees::{Column, Entity};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    MegaIssue,
    MegaCl,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::MegaIssue => Entity::belongs_to(crate::mega_issue::Entity)
                .from(Column::ItemId)
                .to(crate::mega_issue::Column::Id)
                .into(),
            Self::MegaCl => Entity::belongs_to(crate::mega_cl::Entity)
                .from(Column::ItemId)
                .to(crate::mega_cl::Column::Id)
                .into(),
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
