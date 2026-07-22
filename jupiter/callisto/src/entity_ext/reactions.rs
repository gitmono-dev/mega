use sea_orm::entity::prelude::*;

use crate::{
    entity_ext::{generate_id, generate_public_id},
    reactions::{self, Column, Entity},
};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Conversation,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Conversation => Entity::belongs_to(crate::mega_conversation::Entity)
                .from(Column::SubjectId)
                .to(crate::mega_conversation::Column::Id)
                .into(),
        }
    }
}

impl Related<crate::mega_conversation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Conversation.def()
    }
}

impl reactions::Model {
    pub fn new(
        content: Option<String>,
        subject_id: i64,
        subject_type: &str,
        username: &str,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: generate_id(),
            created_at: now,
            updated_at: now,
            public_id: generate_public_id(),
            content,
            subject_id,
            subject_type: subject_type.to_owned(),
            organization_membership_id: None,
            username: username.to_owned(),
            discarded_at: None,
        }
    }
}
