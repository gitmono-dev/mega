use callisto::label;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::serde_snowflake::{
    deserialize_i64_from_string_or_number, deserialize_vec_i64_from_string_or_number,
    serialize_i64_as_string,
};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LabelItem {
    #[serde(
        serialize_with = "serialize_i64_as_string",
        deserialize_with = "deserialize_i64_from_string_or_number"
    )]
    #[schema(value_type = String)]
    pub id: i64,
    pub name: String,
    pub color: String,
    pub description: String,
}

impl From<label::Model> for LabelItem {
    fn from(value: label::Model) -> Self {
        Self {
            id: value.id,
            name: value.name,
            color: value.color,
            description: value.description,
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct NewLabel {
    pub name: String,
    pub color: String,
    pub description: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LabelUpdatePayload {
    #[serde(deserialize_with = "deserialize_vec_i64_from_string_or_number")]
    #[schema(value_type = Vec<String>)]
    pub label_ids: Vec<i64>,
    #[serde(deserialize_with = "deserialize_i64_from_string_or_number")]
    #[schema(value_type = String)]
    pub item_id: i64,
    pub link: String,
}

#[cfg(test)]
mod tests {
    use super::LabelItem;

    #[test]
    fn label_item_id_serializes_as_json_string() {
        let item = LabelItem {
            id: 13_502_510_928_822_277,
            name: "bug".into(),
            color: "#f00".into(),
            description: String::new(),
        };
        let value = serde_json::to_value(&item).unwrap();
        match &value["id"] {
            serde_json::Value::String(s) => assert_eq!(s, "13502510928822277"),
            other => panic!("expected JSON string id, got {other:?}"),
        }
    }
}
