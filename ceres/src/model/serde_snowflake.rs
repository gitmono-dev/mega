//! Serialize snowflake `i64` IDs as JSON strings so JS clients keep full precision
//! (`Number.MAX_SAFE_INTEGER` is 2^53-1; snowflake IDs exceed that).

use std::{fmt, ops::Deref, str::FromStr};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
    ser::SerializeSeq,
};
use utoipa::ToSchema;

/// Path/body snowflake id: OpenAPI + JSON as string, runtime `i64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ToSchema)]
#[schema(value_type = String)]
pub struct SnowflakeId(pub i64);

impl SnowflakeId {
    pub fn get(self) -> i64 {
        self.0
    }
}

impl From<i64> for SnowflakeId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<SnowflakeId> for i64 {
    fn from(value: SnowflakeId) -> Self {
        value.0
    }
}

impl Deref for SnowflakeId {
    type Target = i64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for SnowflakeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SnowflakeId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl Serialize for SnowflakeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_i64_as_string(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for SnowflakeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_i64_from_string_or_number(deserializer).map(Self)
    }
}

pub fn serialize_i64_as_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub fn deserialize_i64_from_string_or_number<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct SnowflakeVisitor;

    impl<'de> Visitor<'de> for SnowflakeVisitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an i64 snowflake id as a string or number")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i64::try_from(value).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse().map_err(E::custom)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(SnowflakeVisitor)
}

pub fn serialize_option_i64_as_string<S>(
    value: &Option<i64>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(v) => serializer.serialize_some(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize_option_i64_from_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptVisitor;

    impl<'de> Visitor<'de> for OptVisitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional i64 snowflake id as a string or number")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_i64_from_string_or_number(deserializer).map(Some)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i64::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse().map(Some).map_err(E::custom)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(OptVisitor)
}

pub fn serialize_vec_i64_as_string<S>(value: &[i64], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = serializer.serialize_seq(Some(value.len()))?;
    for id in value {
        seq.serialize_element(&id.to_string())?;
    }
    seq.end()
}

pub fn deserialize_vec_i64_from_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Vec<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VecVisitor;

    impl<'de> Visitor<'de> for VecVisitor {
        type Value = Vec<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a sequence of i64 snowflake ids as strings or numbers")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match value {
                    serde_json::Value::Number(n) => {
                        let id = n
                            .as_i64()
                            .or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok()))
                            .ok_or_else(|| de::Error::custom("invalid numeric snowflake id"))?;
                        out.push(id);
                    }
                    serde_json::Value::String(s) => {
                        out.push(s.parse().map_err(de::Error::custom)?);
                    }
                    other => {
                        return Err(de::Error::custom(format!(
                            "expected string or number snowflake id, got {other}"
                        )));
                    }
                }
            }
            Ok(out)
        }
    }

    deserializer.deserialize_seq(VecVisitor)
}

pub fn deserialize_option_vec_i64_from_string_or_number<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<i64>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptVecVisitor;

    impl<'de> Visitor<'de> for OptVecVisitor {
        type Value = Option<Vec<i64>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional sequence of i64 snowflake ids")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_vec_i64_from_string_or_number(deserializer).map(Some)
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            // Direct array without Option wrapper.
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            let mut seq = seq;
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match value {
                    serde_json::Value::Number(n) => {
                        let id = n
                            .as_i64()
                            .or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok()))
                            .ok_or_else(|| de::Error::custom("invalid numeric snowflake id"))?;
                        out.push(id);
                    }
                    serde_json::Value::String(s) => {
                        out.push(s.parse().map_err(de::Error::custom)?);
                    }
                    other => {
                        return Err(de::Error::custom(format!(
                            "expected string or number snowflake id, got {other}"
                        )));
                    }
                }
            }
            Ok(Some(out))
        }
    }

    deserializer.deserialize_any(OptVecVisitor)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use super::*;

    /// Post-#2177 snowflake that exceeds JS `Number.MAX_SAFE_INTEGER`.
    const UNSAFE_ID: i64 = 13_502_510_928_822_277;
    /// Pre-#2177 magnitude; still fits in a JS Number.
    const SAFE_ID: i64 = 3_306_264_941_936_901;

    #[derive(Serialize, Deserialize)]
    struct IdField {
        #[serde(
            serialize_with = "serialize_i64_as_string",
            deserialize_with = "deserialize_i64_from_string_or_number"
        )]
        id: i64,
    }

    #[derive(Serialize, Deserialize)]
    struct OptIdField {
        #[serde(
            serialize_with = "serialize_option_i64_as_string",
            deserialize_with = "deserialize_option_i64_from_string_or_number"
        )]
        id: Option<i64>,
    }

    #[derive(Serialize, Deserialize)]
    struct VecIdField {
        #[serde(
            serialize_with = "serialize_vec_i64_as_string",
            deserialize_with = "deserialize_vec_i64_from_string_or_number"
        )]
        ids: Vec<i64>,
    }

    #[test]
    fn json_number_loses_precision_above_js_max_safe() {
        let as_number = serde_json::to_string(&UNSAFE_ID).unwrap();
        assert!(
            !as_number.contains('"'),
            "bare i64 serializes as a JSON number"
        );
        let as_f64: f64 = serde_json::from_str(&as_number).unwrap();
        assert_ne!(
            as_f64 as i64, UNSAFE_ID,
            "parsing the JSON number as f64 (JS Number) must lose precision"
        );
        assert_ne!(UNSAFE_ID, UNSAFE_ID as f64 as i64);
    }

    #[test]
    fn serialize_i64_as_string_preserves_unsafe_id() {
        let value = serde_json::to_value(IdField { id: UNSAFE_ID }).unwrap();
        match &value["id"] {
            Value::String(s) => assert_eq!(s, "13502510928822277"),
            other => panic!("expected JSON string id, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_accepts_string_or_safe_number() {
        let from_string: IdField = serde_json::from_str(r#"{"id":"13502510928822277"}"#).unwrap();
        assert_eq!(from_string.id, UNSAFE_ID);

        let from_number: IdField = serde_json::from_str(r#"{"id":3306264941936901}"#).unwrap();
        assert_eq!(from_number.id, SAFE_ID);
    }

    #[test]
    fn string_round_trip_preserves_unsafe_id() {
        let json = serde_json::to_string(&IdField { id: UNSAFE_ID }).unwrap();
        let back: IdField = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, UNSAFE_ID);
    }

    #[test]
    fn option_and_vec_serialize_unsafe_ids_as_strings() {
        let opt = serde_json::to_value(OptIdField {
            id: Some(UNSAFE_ID),
        })
        .unwrap();
        assert!(matches!(opt["id"], Value::String(_)));
        assert_eq!(opt["id"], "13502510928822277");

        let none = serde_json::to_value(OptIdField { id: None }).unwrap();
        assert!(none["id"].is_null());

        let vec = serde_json::to_value(VecIdField {
            ids: vec![UNSAFE_ID, SAFE_ID],
        })
        .unwrap();
        assert_eq!(
            vec["ids"],
            Value::Array(vec![
                Value::String("13502510928822277".into()),
                Value::String("3306264941936901".into()),
            ])
        );

        let back: VecIdField = serde_json::from_value(vec).unwrap();
        assert_eq!(back.ids, vec![UNSAFE_ID, SAFE_ID]);
    }
}
