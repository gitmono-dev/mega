use sea_orm::entity::prelude::*;

use crate::{
    entity_ext::generate_id,
    mega_code_review_thread::{self, Column, Entity},
    sea_orm_active_enums::ThreadStatusEnum,
};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    MegaCl,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::MegaCl => Entity::belongs_to(crate::mega_cl::Entity)
                .from(Column::Link)
                .to(crate::mega_cl::Column::Link)
                .into(),
        }
    }
}

impl Related<crate::mega_cl::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MegaCl.def()
    }
}

impl mega_code_review_thread::Model {
    pub fn new(link: &str, thread_status: ThreadStatusEnum) -> Self {
        let now = chrono::Utc::now().naive_utc();

        Self {
            id: generate_id(),
            link: link.to_owned(),
            thread_status,
            created_at: now,
            updated_at: now,
        }
    }
}
