use std::collections::{HashMap, HashSet};

use callisto::{mega_conversation, reactions, sea_orm_active_enums::ConvTypeEnum};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ConversationItem {
    pub id: i64,
    pub username: String,
    /// True when `username` matches a registered bot (or legacy `"system"` actor).
    #[serde(default)]
    pub is_bot: bool,
    pub conv_type: ConvType,
    pub comment: Option<String>,
    pub resolved: Option<bool>,
    pub created_at: i64,
    pub updated_at: i64,
    pub grouped_reactions: Vec<ReactionItem>,
}

impl ConversationItem {
    pub fn from_model(
        conversation: mega_conversation::Model,
        reactions: Vec<reactions::Model>,
        viewer: &str,
    ) -> Self {
        let mut item = Self {
            id: conversation.id,
            username: conversation.campsite_user_id,
            is_bot: false,
            conv_type: conversation.conv_type.into(),
            comment: conversation.comment,
            resolved: conversation.resolved,
            created_at: conversation.created_at.and_utc().timestamp(),
            updated_at: conversation.updated_at.and_utc().timestamp(),
            grouped_reactions: vec![],
        };
        item.grouped_emoji(viewer, reactions);
        item
    }

    /// Mark conversation actors that are bots (or the legacy `"system"` merge actor).
    pub fn apply_bot_flags(items: &mut [Self], bot_names: &HashSet<String>) {
        for item in items {
            item.is_bot = bot_names.contains(&item.username) || item.username == "system";
        }
    }

    pub fn grouped_emoji(&mut self, username: &str, reactions: Vec<reactions::Model>) {
        let mut reactions_map: HashMap<String, ReactionItem> = HashMap::new();
        let username = username.to_owned();
        for r in &reactions {
            if let Some(emoji) = &r.content {
                let entry = reactions_map
                    .entry(emoji.clone())
                    .or_insert_with(|| ReactionItem {
                        emoji: emoji.clone(),
                        reactions_count: 0,
                        viewer_reaction_id: String::new(),
                        tooltip: Vec::new(),
                        custom_content: String::new(),
                    });
                entry.reactions_count += 1;
                if r.campsite_user_id == username {
                    entry.viewer_reaction_id = r.public_id.clone();
                }
                entry.tooltip.push(r.campsite_user_id.clone());
            }
        }

        self.grouped_reactions = reactions_map.into_values().collect();
    }
}

#[derive(Serialize, Default, ToSchema)]
pub struct ReactionItem {
    pub viewer_reaction_id: String,
    pub emoji: String,
    pub tooltip: Vec<String>,
    pub reactions_count: usize,
    pub custom_content: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ContentPayload {
    pub content: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ReactionRequest {
    pub content: String,
    pub comment_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ConvType {
    Comment,
    Deploy,
    Commit,
    ForcePush,
    Edit,
    Review,
    Approve,
    MergeQueue,
    Merged,
    Closed,
    Reopen,
    Label,
    Assignee,
    Mention,
    Draft,
}

impl From<ConvTypeEnum> for ConvType {
    fn from(value: ConvTypeEnum) -> Self {
        match value {
            ConvTypeEnum::Comment => ConvType::Comment,
            ConvTypeEnum::Deploy => ConvType::Deploy,
            ConvTypeEnum::Commit => ConvType::Commit,
            ConvTypeEnum::ForcePush => ConvType::ForcePush,
            ConvTypeEnum::Edit => ConvType::Edit,
            ConvTypeEnum::Review => ConvType::Review,
            ConvTypeEnum::Approve => ConvType::Approve,
            ConvTypeEnum::MergeQueue => ConvType::MergeQueue,
            ConvTypeEnum::Merged => ConvType::Merged,
            ConvTypeEnum::Closed => ConvType::Closed,
            ConvTypeEnum::Reopen => ConvType::Reopen,
            ConvTypeEnum::Label => ConvType::Label,
            ConvTypeEnum::Assignee => ConvType::Assignee,
            ConvTypeEnum::Mention => ConvType::Mention,
            ConvTypeEnum::Draft => ConvType::Draft,
        }
    }
}

impl From<ConvType> for ConvTypeEnum {
    fn from(value: ConvType) -> Self {
        match value {
            ConvType::Comment => ConvTypeEnum::Comment,
            ConvType::Deploy => ConvTypeEnum::Deploy,
            ConvType::Commit => ConvTypeEnum::Commit,
            ConvType::ForcePush => ConvTypeEnum::ForcePush,
            ConvType::Edit => ConvTypeEnum::Edit,
            ConvType::Review => ConvTypeEnum::Review,
            ConvType::Approve => ConvTypeEnum::Approve,
            ConvType::MergeQueue => ConvTypeEnum::MergeQueue,
            ConvType::Merged => ConvTypeEnum::Merged,
            ConvType::Closed => ConvTypeEnum::Closed,
            ConvType::Reopen => ConvTypeEnum::Reopen,
            ConvType::Label => ConvTypeEnum::Label,
            ConvType::Assignee => ConvTypeEnum::Assignee,
            ConvType::Mention => ConvTypeEnum::Mention,
            ConvType::Draft => ConvTypeEnum::Draft,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceType {
    Mention,
    BuildRelates,
    Blocks,
}

impl From<callisto::sea_orm_active_enums::ReferenceTypeEnum> for ReferenceType {
    fn from(value: callisto::sea_orm_active_enums::ReferenceTypeEnum) -> Self {
        use callisto::sea_orm_active_enums::ReferenceTypeEnum;
        match value {
            ReferenceTypeEnum::Mention => ReferenceType::Mention,
            ReferenceTypeEnum::BuildRelates => ReferenceType::BuildRelates,
            ReferenceTypeEnum::Blocks => ReferenceType::Blocks,
        }
    }
}

impl From<ReferenceType> for callisto::sea_orm_active_enums::ReferenceTypeEnum {
    fn from(value: ReferenceType) -> Self {
        use callisto::sea_orm_active_enums::ReferenceTypeEnum;
        match value {
            ReferenceType::Mention => ReferenceTypeEnum::Mention,
            ReferenceType::BuildRelates => ReferenceTypeEnum::BuildRelates,
            ReferenceType::Blocks => ReferenceTypeEnum::Blocks,
        }
    }
}
