//! Serialization and `PostgREST` integration for generated database bindings.

use serde::{Deserialize, Serialize, Serializer};

mod json;

pub use json::{NonNullJson, NullJson};

/// A write field that distinguishes an absent key from an explicit value.
/// Use `Field<Option<T>>` when the database permits an explicit JSON null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Field<T> {
    /// Leave the key out of the request, preserving database defaults.
    #[default]
    Omit,
    /// Send this value (including null when `T` is an `Option`).
    Value(T),
}

impl<T> Field<T> {
    /// Whether a generated serializer should omit this field.
    #[must_use]
    pub const fn is_omit(&self) -> bool {
        matches!(self, Self::Omit)
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Omit => Err(serde::ser::Error::custom(
                "Field::Omit must be skipped by the containing field serializer",
            )),
        }
    }
}

/// `PostgreSQL` arrays support nullable elements and variable rank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Array<T> {
    /// A one-dimensional array, including null elements.
    Elements(Vec<Option<T>>),
    /// A higher-dimensional array.
    Nested(Vec<Self>),
}

/// A generated relation's exact database identity.
pub trait Relation {
    /// `PostgreSQL` schema name.
    const SCHEMA: &'static str;
    /// `PostgreSQL` relation name.
    const NAME: &'static str;
}

/// A generated function's identity and request/response types.
pub trait Function {
    /// Serializable RPC argument object.
    type Args: Serialize;
    /// Response decoded using `serde_json`.
    type Returns;
    /// `PostgreSQL` schema name.
    const SCHEMA: &'static str;
    /// `PostgreSQL` function name.
    const NAME: &'static str;
}

pub mod params;
mod projection;
pub use params::{Nulls, Order, QueryPair};
pub use projection::{FilterColumn, SharedFilter, SharedFilterFor};
mod query;
mod relationship;
pub mod selection;
pub use selection::{Selection, named};
/// One Unicode scalar in a lossless generated identifier key.
pub struct Character<const C: char>;
/// A zero-sized identifier key, independent of the selected value.
pub struct Key<Name>(core::marker::PhantomData<fn() -> Name>);
impl<Name> Copy for Key<Name> {}
impl<Name> Clone for Key<Name> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<Name> Key<Name> {
    /// Construct a zero-sized identifier key.
    #[must_use]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<Name> Default for Key<Name> {
    fn default() -> Self {
        Self::new()
    }
}
/// The identifier a macro lookup was written with, and its schema key.
/// Macros implement it for a unit struct named after the identifier, so that
/// lookup diagnostics print the identifier as a word instead of the encoded key.
#[doc(hidden)]
pub trait KeyName {
    /// The lossless key that `key!(type ...)` also produces.
    type Key;
}
/// Resolve a normalized column identifier on this exact relation.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no column for this key",
    label = "unknown column"
)]
pub trait ColumnByKey<Name>: Relation {
    /// Generated column marker.
    type Column: Column<Relation = Self>;
    /// Zero-sized column value.
    const COLUMN: Self::Column;
}
/// Resolve a normalized relationship identifier on this exact relation.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no relationship for this key",
    label = "unknown relationship"
)]
pub trait RelationshipByKey<Name>: Relation {
    /// Generated relationship marker.
    type Edge: Relationship<Source = Self>;
}
pub use query::{
    Column, JsonColumn, Locked, NullableColumn, Paged, Projection, Query, Read, ScopedFilters,
    Unlocked, WritableRelation, Write, query,
};
pub use relationship::{
    Cardinality, Embed, EmbedPath, EmptySelection, Path, Relationship, ToMany, ToOne,
};
pub use rp_postgrest::{Count, Counted, Postgrest};

#[doc(hidden)]
pub mod __private {
    pub use super::projection::assert_same_filter;
    pub use super::relationship::{
        alias, assert_distinct, assert_same_column, check_embed, check_empty, identifier_len,
        write_identifier,
    };
    pub use rp_supabase_client_macros::__projection;
    pub use serde;
}

/// A typed RPC request with an optional server single-object response mode.
#[must_use]
pub struct Rpc<F: Function> {
    builder: rp_postgrest::Builder,
    marker: core::marker::PhantomData<fn() -> F>,
}

impl<F: Function> Rpc<F> {
    /// Request server-enforced single-object cardinality.
    ///
    /// This keeps the generated return type unchanged. For a set-returning
    /// function, use `single().fetch_as::<Row>()` to decode the selected object
    /// rather than the generated collection type.
    pub fn single(mut self) -> Self {
        self.builder = self.builder.single();
        self
    }

    /// Drop the function's return-type guarantee for raw protocol composition.
    pub fn into_raw(self) -> rp_postgrest::Builder {
        self.builder
    }

    /// Execute and decode the generated function's return type.
    ///
    /// # Errors
    /// Preserves serialization, transport and response failures.
    pub async fn fetch(self) -> Result<F::Returns, rp_postgrest::Error>
    where
        F::Returns: serde::de::DeserializeOwned,
    {
        self.builder.fetch().await
    }

    /// Execute and decode JSON as an explicitly selected response type.
    ///
    /// Use this for a typed JSON/JSONB result or a set-returning function in
    /// single-object mode. [`Self::fetch`] retains the generated return type.
    ///
    /// # Errors
    /// Preserves serialization, transport, server and response decoding failures.
    pub async fn fetch_as<T: serde::de::DeserializeOwned>(self) -> Result<T, rp_postgrest::Error> {
        self.builder.fetch().await
    }

    /// Execute and check the response status without decoding a successful body.
    ///
    /// # Errors
    /// Preserves serialization, transport and server-response failures.
    pub async fn execute(self) -> Result<(), rp_postgrest::Error> {
        self.builder.execute_checked().await?;
        Ok(())
    }
}

/// Start an RPC request, deferring argument serialization failures to execution.
#[expect(
    clippy::needless_pass_by_value,
    reason = "RPC construction uses the same consuming client convention as generated queries"
)]
pub fn rpc<F: Function>(client: rp_postgrest::Postgrest, args: &F::Args) -> Rpc<F> {
    Rpc {
        builder: client.rpc_json(F::NAME, args).schema(F::SCHEMA),
        marker: core::marker::PhantomData,
    }
}

/// Include bindings produced by a consumer's build script in `OUT_DIR`.
#[macro_export]
macro_rules! include_schema {
    ($filename:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $filename));
    };
}

/// Common generated binding runtime imports.
pub mod prelude {
    pub use super::{
        Array, Column, Count, Field, Function, JsonColumn, NullableColumn, Nulls, Order, Paged,
        Projection, Query, Relation, Rpc, Selection, WritableRelation, named, query, rpc,
    };
    pub use crate::{include_schema, key, projection, select};
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "Test failures should retain the underlying serialization error."
)]
mod tests {
    use super::{Array, Field};
    use serde::Serialize;
    use serde_json::{Value, json};

    #[derive(Default, Serialize)]
    struct Write {
        #[serde(skip_serializing_if = "Field::is_omit")]
        nullable: Field<Option<String>>,
        #[serde(skip_serializing_if = "Field::is_omit")]
        required: Field<String>,
    }

    #[test]
    fn omission_null_and_values_are_distinct() {
        assert_eq!(serde_json::to_value(Write::default()).unwrap(), json!({}));
        let mut write = Write {
            nullable: Field::Value(None),
            ..Write::default()
        };
        assert_eq!(
            serde_json::to_value(&write).unwrap(),
            json!({"nullable": null})
        );
        write.nullable = Field::Value(Some("present".into()));
        write.required = Field::Value("required".into());
        assert_eq!(
            serde_json::to_value(write).unwrap(),
            json!({"nullable": "present", "required": "required"})
        );
    }

    #[test]
    fn omission_outside_a_skipped_field_is_an_error() {
        serde_json::to_value(Field::<String>::Omit).unwrap_err();
        serde_json::to_value(vec![Field::<String>::Omit]).unwrap_err();
        assert_eq!(
            serde_json::to_value(Field::<Option<String>>::Value(None)).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn arrays_preserve_rank_null_elements_and_empty_arrays() {
        for input in [
            json!([]),
            json!([1_i32, null, 3_i32]),
            json!([[1_i32, null], [2_i32, 3_i32]]),
            json!([[[], []], [[], []]]),
        ] {
            let array: Array<i32> = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(array).unwrap(), input);
        }
        serde_json::from_value::<Array<i32>>(json!([1_i32, [2_i32]])).unwrap_err();
        serde_json::from_value::<Array<i32>>(Value::Null).unwrap_err();
    }
}
