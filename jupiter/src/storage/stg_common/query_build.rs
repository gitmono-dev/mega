use std::collections::HashMap;

use callisto::{item_assignees, item_labels};
use sea_orm::{ColumnTrait, Condition, Order, QueryOrder};

pub fn filter_by_labels(cond: Condition, labels: Option<Vec<i64>>) -> Condition {
    if let Some(value) = labels {
        cond.add(item_labels::Column::LabelId.is_in(value))
    } else {
        cond
    }
}

pub fn filter_by_assignees(cond: Condition, assignees: Option<Vec<String>>) -> Condition {
    if let Some(value) = assignees {
        cond.add(item_assignees::Column::CampsiteUserId.is_in(value))
    } else {
        cond
    }
}

/// Apply order_by dynamically based on user input.
pub fn apply_sort<C, Q>(
    mut query: Q,
    sort_by: Option<&str>,
    asc: bool,
    columns: &HashMap<&str, C>,
) -> Q
where
    C: ColumnTrait + Copy,
    Q: QueryOrder + Sized,
{
    if let Some(field) = sort_by
        && let Some(column) = columns.get(field)
    {
        let order = if asc { Order::Asc } else { Order::Desc };
        query = query.order_by(*column, order);
    }
    query
}
