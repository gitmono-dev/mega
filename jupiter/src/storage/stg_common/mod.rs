use std::collections::HashMap;

use callisto::{item_assignees, label, mega_conversation, sea_orm_active_enums::ConvTypeEnum};
use indexmap::IndexMap;

use crate::{model::common::ItemDetails, storage::stg_common::item::ItemEntity};

pub mod item;
pub mod query_build;

/// Combine labels, assignees, and conversations into a unified list of `ItemDetails`.
///
/// This function merges multiple related datasets for a list of issues:
/// - `item_labels`: pairs of (issue, list of labels)
/// - `item_assignees`: pairs of (issue, list of assignees)
/// - `conversations`: pairs of (issue, list of conversations)
///
/// It aggregates the data into a single `ItemDetails` structure for each issue,
/// ensuring that even if some parts are missing (e.g. labels or assignees),
/// a complete `ItemDetails` entry is still created.
///
/// # Arguments
///
/// * `item_labels` - A vector of tuples where each tuple contains an issue and its associated labels.
/// * `item_assignees` - A vector of tuples where each tuple contains an issue and its associated assignees.
/// * `conversations` - A vector of tuples where each tuple contains an issue and its associated conversations.
///
/// # Returns
///
/// A vector of `ItemDetails` combining the data from the above sources.
pub fn combine_item_list<T>(
    item_labels: Vec<(T::Model, Vec<label::Model>)>,
    item_assignees: Vec<(T::Model, Vec<item_assignees::Model>)>,
    conversations: Vec<(T::Model, Vec<mega_conversation::Model>)>,
) -> Vec<ItemDetails>
where
    T: ItemEntity,
    T::Model: Clone,
{
    let mut conv_map = HashMap::new();
    for (model, convs) in conversations {
        let id = T::get_id(&model);
        conv_map.insert(
            id,
            convs
                .into_iter()
                .filter(|m| m.conv_type == ConvTypeEnum::Comment)
                .collect::<Vec<_>>()
                .len(),
        );
    }

    let mut result: IndexMap<i64, ItemDetails> = IndexMap::new();
    for (model, labels) in item_labels {
        let id = T::get_id(&model);
        result.insert(
            id,
            ItemDetails {
                item: T::item_kind(model),
                labels,
                assignees: vec![],
                comment_num: *conv_map.get(&id).unwrap_or(&0),
            },
        );
    }

    for (model, assignees) in item_assignees {
        let id = T::get_id(&model);
        let assignees = assignees
            .iter()
            .map(|m| m.campsite_user_id.clone())
            .collect();
        if let Some(entry) = result.get_mut(&id) {
            entry.assignees = assignees;
        } else {
            result.insert(
                id,
                ItemDetails {
                    item: T::item_kind(model),
                    labels: vec![],
                    assignees,
                    comment_num: *conv_map.get(&id).unwrap_or(&0),
                },
            );
        }
    }

    result.into_values().collect()
}

#[cfg(test)]
mod tests {
    use callisto::{
        item_assignees, label, mega_conversation, mega_issue, sea_orm_active_enums::ConvTypeEnum,
    };

    use super::*;

    #[test]
    fn test_combine_item_list() {
        let issue = mega_issue::Model {
            id: 1,
            link: String::from("ILD2EV5V"),
            title: String::from("[Monobean] no such column: mega_refs.is_cl #1028"),
            status: String::from("open"),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            closed_at: None,
            campsite_user_id: String::from("benjamin_747"),
        };

        let label = label::Model {
            id: 1,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            name: String::from("bugs"),
            color: String::from("#000000"),
            description: String::from("des"),
        };

        let assignee = item_assignees::Model {
            item_id: 1,
            campsite_user_id: "alice".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            item_type: String::from("issue"),
        };

        let conv = mega_conversation::Model {
            id: 1,
            conv_type: ConvTypeEnum::Comment,
            link: String::from("ILD2EV5V"),
            comment: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            campsite_user_id: String::from("benjamin_747"),
            resolved: None,
        };

        let item_labels = vec![(issue.clone(), vec![label])];
        let item_assignees = vec![(issue.clone(), vec![assignee])];
        let conversations = vec![(issue.clone(), vec![conv])];

        let results =
            combine_item_list::<mega_issue::Entity>(item_labels, item_assignees, conversations);

        assert_eq!(results.len(), 1);
        let details = &results[0];
        assert_eq!(details.comment_num, 1);
        assert_eq!(details.labels.len(), 1);
        assert_eq!(details.assignees, vec!["alice"]);
    }
}
