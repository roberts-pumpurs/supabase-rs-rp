#![cfg_attr(all(doc, not(doctest)), doc = include_str!("../README.md"))]

extern crate alloc;

#[cfg(feature = "database")]
mod database;
mod emitter;
pub mod model;

use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use model::{SNAPSHOT_VERSION, Snapshot};

/// Generation or schema acquisition failed.
#[expect(
    clippy::error_impl_error,
    reason = "The module-qualified codegen::Error is unambiguous for callers."
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid schema generation input: {0}")]
    Invalid(String),
    #[error("schema generation I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid schema snapshot: {0}")]
    Json(#[from] serde_json::Error),
    #[error("PostgreSQL schema introspection failed: {0}")]
    Database(String),
}

#[derive(Debug, Clone)]
struct Config {
    pub schemas: Vec<String>,
    pub prelude: String,
    pub derives: Vec<String>,
    pub attributes: Vec<String>,
    pub type_overrides: BTreeMap<String, String>,
    pub type_attributes: BTreeMap<String, Vec<String>>,
    pub runtime_path: String,
    pub not_null: BTreeMap<String, Vec<String>>,
    pub column_types: BTreeMap<String, String>,
    pub json_types: BTreeMap<String, String>,
    pub reexport_macros: bool,
    pub relationship_aliases: BTreeMap<String, String>,
    pub strict_args: bool,
    pub strict_functions: BTreeSet<String>,
}

/// Configure code generation, then choose one explicit schema source.
///
/// Custom Rust syntax is checked before writing output. Generated structs preserve SQL names
/// with Serde attributes. Additional derives and attributes apply to data types, not markers.
#[derive(Debug, Clone)]
pub struct Generator {
    config: Config,
}

impl Default for Generator {
    fn default() -> Self {
        Self::new()
    }
}

#[expect(
    clippy::impl_trait_in_params,
    reason = "Consuming configuration methods accept owned strings and borrowed literals without named generic parameters."
)]
#[expect(
    clippy::wrong_self_convention,
    reason = "Each from_* method consumes configured generation options and selects an input adapter."
)]
impl Generator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: Config {
                schemas: vec!["public".to_owned()],
                prelude: String::new(),
                derives: Vec::new(),
                attributes: Vec::new(),
                type_overrides: BTreeMap::new(),
                type_attributes: BTreeMap::new(),
                runtime_path: "::rp_supabase_client::schema".to_owned(),
                not_null: BTreeMap::new(),
                column_types: BTreeMap::new(),
                json_types: BTreeMap::new(),
                reexport_macros: false,
                relationship_aliases: BTreeMap::new(),
                strict_args: false,
                strict_functions: BTreeSet::new(),
            },
        }
    }

    /// Replace the default `public` selection with these schemas.
    #[must_use]
    pub fn schemas(mut self, schemas: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.config.schemas = schemas.into_iter().map(Into::into).collect();
        self
    }

    /// Select a single schema instead of the default `public`.
    #[must_use]
    pub fn schema(self, schema: impl Into<String>) -> Self {
        self.schemas([schema.into()])
    }

    /// Add Rust items or imports before the generated schema modules.
    #[must_use]
    pub fn prelude(mut self, prelude: impl Into<String>) -> Self {
        self.config.prelude = prelude.into();
        self
    }

    /// Add a derive path, such as `::typed_builder::TypedBuilder`.
    #[must_use]
    pub fn derive(mut self, derive: impl Into<String>) -> Self {
        self.config.derives.push(derive.into());
        self
    }

    /// Add an outer attribute, including its `#[...]` delimiters.
    #[must_use]
    pub fn attribute(mut self, attribute: impl Into<String>) -> Self {
        self.config.attributes.push(attribute.into());
        self
    }

    /// Add an attribute to one generated data type.
    ///
    /// Paths use generated Rust names, for example `public.tables.messages.Insert`.
    /// Unknown paths fail generation instead of silently ignoring customization.
    #[must_use]
    pub fn type_attribute(
        mut self,
        generated_type: impl Into<String>,
        attribute: impl Into<String>,
    ) -> Self {
        self.config
            .type_attributes
            .entry(generated_type.into())
            .or_default()
            .push(attribute.into());
        self
    }

    /// Map a qualified SQL type to a Rust type that matches the `PostgREST` JSON representation.
    #[must_use]
    pub fn type_override(
        mut self,
        postgres_type: impl Into<String>,
        rust_type: impl Into<String>,
    ) -> Self {
        self.config
            .type_overrides
            .insert(postgres_type.into(), rust_type.into());
        self
    }

    /// Set the runtime module path when the client dependency has a Cargo alias.
    #[must_use]
    pub fn runtime_path(mut self, path: impl Into<String>) -> Self {
        self.config.runtime_path = path.into();
        self
    }

    /// Assert non-null fields on a view Row, composite, or RPC Record.
    /// Unknown targets and SQL field names fail generation.
    #[must_use]
    pub fn not_null(
        mut self,
        target: impl Into<String>,
        fields: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.config
            .not_null
            .insert(target.into(), fields.into_iter().map(Into::into).collect());
        self
    }

    /// Override one SQL column's base type, preserving nullability and omission.
    /// Accepts `public.table.column` or `public.tables.table.column`.
    #[must_use]
    pub fn column_type(mut self, target: impl Into<String>, rust_type: impl Into<String>) -> Self {
        self.config
            .column_types
            .insert(target.into(), rust_type.into());
        self
    }

    /// Decode a JSON field, RPC argument or return using a custom Serde type.
    /// SQL arrays, nullability, and set-returning wrappers remain intact.
    #[must_use]
    pub fn json_type(mut self, target: impl Into<String>, rust_type: impl Into<String>) -> Self {
        self.config
            .json_types
            .insert(target.into(), rust_type.into());
        self
    }

    /// Export crate-local `select!` and `key!` wrappers with the runtime path filled.
    /// Include the generated bindings at the schema crate's root.
    #[must_use]
    pub const fn reexport_macros(mut self) -> Self {
        self.config.reexport_macros = true;
        self
    }

    /// Add an alias for one canonical qualified relationship marker.
    /// For example `public.tables.orders.relationships.orders_customer_fkey`.
    #[must_use]
    pub fn relationship_alias(
        mut self,
        target: impl Into<String>,
        alias: impl Into<String>,
    ) -> Self {
        self.config
            .relationship_aliases
            .insert(target.into(), alias.into());
        self
    }

    /// Require non-default RPC inputs unless explicitly marked nullable.
    #[must_use]
    pub const fn strict_args(mut self) -> Self {
        self.config.strict_args = true;
        self
    }

    /// Apply strict RPC input contracts to one generated function module.
    #[must_use]
    pub fn strict_args_for(mut self, target: impl Into<String>) -> Self {
        self.config.strict_functions.insert(target.into());
        self
    }

    /// Load a portable snapshot and register Cargo's file change detection.
    ///
    /// # Errors
    /// Fails for unreadable, invalid, or unsupported-version snapshots.
    #[expect(
        clippy::print_stdout,
        reason = "Cargo build scripts receive change instructions on stdout."
    )]
    pub fn from_snapshot(self, path: impl AsRef<Path>) -> Result<Bindings, Error> {
        let path = path.as_ref();
        cargo_path(path)?;
        println!("cargo::rerun-if-changed={}", path.display());
        let snapshot = parse_snapshot(&std::fs::read(path)?)?;
        self.from_metadata(snapshot)
    }

    /// Generate from in-memory metadata. Does not emit Cargo change instructions.
    ///
    /// # Errors
    /// Fails for unsupported versions, invalid metadata, or invalid custom Rust syntax.
    pub fn from_metadata(self, mut snapshot: Snapshot) -> Result<Bindings, Error> {
        validate_snapshot_version(snapshot.version)?;
        if self.config.schemas.is_empty() || self.config.schemas.iter().any(String::is_empty) {
            return Err(Error::Invalid(
                "select at least one nonempty schema".to_owned(),
            ));
        }
        for schema in &self.config.schemas {
            if !snapshot.schemas.iter().any(|item| &item.name == schema) {
                return Err(Error::Invalid(format!(
                    "selected schema {schema:?} is absent from the snapshot"
                )));
            }
        }
        model::apply_not_null(&mut snapshot, &self.config.not_null)?;
        let source = emitter::generate(&snapshot, &self.config)?;
        Ok(Bindings { snapshot, source })
    }

    /// Acquire portable metadata without rendering Rust bindings.
    ///
    /// # Errors
    /// Fails for empty schema selection, connection errors, or invalid catalog metadata.
    #[cfg(feature = "database")]
    pub fn snapshot_from_database(&self, url: &str) -> Result<Snapshot, Error> {
        if self.config.schemas.is_empty() || self.config.schemas.iter().any(String::is_empty) {
            return Err(Error::Invalid(
                "select at least one nonempty schema".to_owned(),
            ));
        }
        database::introspect(url, &self.config.schemas)
    }

    /// Introspect `PostgreSQL` directly. No CLI or Supabase service-role key is needed.
    ///
    /// Register migration paths in your build script, because Cargo cannot detect database DDL.
    /// Remote connections should use `sslmode=require`; certificates and hostnames are verified.
    ///
    /// # Errors
    /// Fails on connection, catalog, unsupported SQL type, or generation errors.
    #[cfg(feature = "database")]
    pub fn from_database(self, url: &str) -> Result<Bindings, Error> {
        let snapshot = self.snapshot_from_database(url)?;
        self.from_metadata(snapshot)
    }

    /// Read a database connection string from a host environment variable.
    ///
    /// Emits `rerun-if-env-changed` without printing the variable's secret value.
    ///
    /// # Errors
    /// Fails if the variable is absent, invalid, or database introspection fails.
    #[cfg(feature = "database")]
    #[expect(
        clippy::print_stdout,
        reason = "Cargo build scripts receive change instructions on stdout."
    )]
    pub fn from_database_env(self, variable: &str) -> Result<Bindings, Error> {
        if variable.is_empty() || variable.contains(['\n', '\r', '=']) {
            return Err(Error::Invalid(
                "invalid database environment variable name".to_owned(),
            ));
        }
        println!("cargo::rerun-if-env-changed={variable}");
        let url = std::env::var(variable).map_err(|_error| {
            Error::Invalid(format!("set {variable} to a PostgreSQL connection string"))
        })?;
        self.from_database(&url)
    }
}

/// Validated generated Rust source and the metadata used to produce it.
#[derive(Debug, Clone)]
pub struct Bindings {
    snapshot: Snapshot,
    source: String,
}

#[expect(
    clippy::impl_trait_in_params,
    reason = "Path arguments accept standard owned and borrowed path types."
)]
impl Bindings {
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub const fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Write source without changing the file when its content is identical.
    ///
    /// # Errors
    /// Fails when reading or writing the output file fails.
    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        write_if_changed(path.as_ref(), self.source.as_bytes())
    }

    /// Write source inside Cargo's `OUT_DIR`. The filename must be a single path component.
    ///
    /// # Errors
    /// Fails outside a build script or if the filename or output path is invalid.
    pub fn write_to_out_dir(&self, filename: &str) -> Result<PathBuf, Error> {
        let path = Path::new(filename);
        if path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(Error::Invalid(
                "OUT_DIR filename must be one normal path component".to_owned(),
            ));
        }
        let output = std::env::var_os("OUT_DIR").ok_or_else(|| {
            Error::Invalid("OUT_DIR is absent; call this method from build.rs".to_owned())
        })?;
        let output = PathBuf::from(output).join(filename);
        self.write_to(&output)?;
        Ok(output)
    }

    /// Export metadata for later offline builds. Call outside build.rs to update a committed snapshot.
    ///
    /// # Errors
    /// Fails when encoding or writing the snapshot fails.
    pub fn write_snapshot(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        self.snapshot.write_to(path)
    }
}

fn validate_snapshot_version(version: u32) -> Result<(), Error> {
    if version != SNAPSHOT_VERSION {
        return Err(Error::Invalid(format!(
            "snapshot version {version} is unsupported; expected {SNAPSHOT_VERSION}; \
             regenerate the snapshot from PostgreSQL with the current supabase-codegen"
        )));
    }
    Ok(())
}

fn parse_snapshot(bytes: &[u8]) -> Result<Snapshot, Error> {
    #[derive(serde::Deserialize)]
    struct VersionHeader {
        version: u32,
    }

    let header: VersionHeader = serde_json::from_slice(bytes)?;
    validate_snapshot_version(header.version)?;
    Ok(serde_json::from_slice(bytes)?)
}

fn cargo_path(path: &Path) -> Result<(), Error> {
    let Some(path) = path.to_str() else {
        return Err(Error::Invalid("Cargo input paths must be UTF-8".to_owned()));
    };
    if path.contains(['\n', '\r']) {
        return Err(Error::Invalid(
            "Cargo input paths cannot contain line breaks".to_owned(),
        ));
    }
    Ok(())
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<(), Error> {
    match std::fs::read(path) {
        Ok(existing) if existing == contents => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Unexpected fixture decoding errors must print their diagnostic."
)]
mod metadata_tests {
    use super::{Error, Generator, parse_snapshot};
    use crate::model::{Snapshot, Table};

    #[test]
    fn legacy_snapshot_rejected_before_missing_relationship_metadata() {
        assert!(matches!(
            parse_snapshot(
                br#"{"version":1,"schemas":[{"name":"public","enums":[],"composites":[],"functions":[],"tables":[{"name":"orders","kind":"table","columns":[]}]}]}"#,
            ),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn in_memory_metadata_rejects_legacy_version() {
        assert!(matches!(
            Generator::default().from_metadata(Snapshot {
                version: 1,
                schemas: Vec::new(),
            }),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn table_relationship_metadata_is_required_even_for_primary_key() {
        let table = serde_json::json!({
            "name": "orders",
            "kind": "table",
            "columns": [],
            "primary_key": null,
            "unique_keys": [],
            "foreign_keys": [],
            "is_partition": false,
            "computed_fields": []
        });
        for field in [
            "primary_key",
            "unique_keys",
            "foreign_keys",
            "is_partition",
            "computed_fields",
        ] {
            let mut incomplete = table.clone();
            incomplete.as_object_mut().expect("object").remove(field);
            assert!(matches!(
                serde_json::from_value::<Table>(incomplete),
                Err(error) if error.classify() == serde_json::error::Category::Data
            ));
        }
        let decoded: Table = serde_json::from_value(table).expect("explicit empty facts are valid");
        assert_eq!(decoded.primary_key, None);
    }
}
