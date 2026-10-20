use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// A JSON value that is never `null`.
///
/// Generated write payloads (Insert, Update, and strict RPC args) use this for
/// NOT NULL `json`/`jsonb` columns. `PostgREST` turns a JSON null in the request
/// body into SQL NULL, which the column rejects.
///
/// ```
/// use rp_supabase_client::schema::NonNullJson;
///
/// assert!(NonNullJson::try_from(serde_json::json!({"a": 1})).is_ok());
/// assert!(NonNullJson::try_from(serde_json::Value::Null).is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct NonNullJson(Value);

impl NonNullJson {
    /// Returns the wrapped JSON value.
    #[must_use]
    pub fn into_inner(self) -> Value {
        self.0
    }
}

/// Error for a JSON null passed where a [`NonNullJson`] is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NullJson;

impl fmt::Display for NullJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON null is not allowed for a NOT NULL JSON column")
    }
}

impl core::error::Error for NullJson {}

impl TryFrom<Value> for NonNullJson {
    type Error = NullJson;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        if value.is_null() {
            Err(NullJson)
        } else {
            Ok(Self(value))
        }
    }
}

// Decoding applies the same null check, so the invariant holds for deserialized payloads.
impl<'de> Deserialize<'de> for NonNullJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl From<Map<String, Value>> for NonNullJson {
    fn from(map: Map<String, Value>) -> Self {
        Self(Value::Object(map))
    }
}

impl From<NonNullJson> for Value {
    fn from(json: NonNullJson) -> Self {
        json.0
    }
}

impl AsRef<Value> for NonNullJson {
    fn as_ref(&self) -> &Value {
        &self.0
    }
}
