//! Export portable PostgreSQL metadata and check a committed snapshot for schema drift.

extern crate alloc;

use alloc::borrow::Cow;
use alloc::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use rp_supabase_codegen::Generator;
use rp_supabase_codegen::model::{SNAPSHOT_VERSION, Snapshot};
use serde_json::Value;

#[derive(Parser)]
#[command(
    version,
    about = "Write and check portable PostgreSQL schema snapshots"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage committed schema metadata independently of Cargo build scripts.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
}

#[derive(Subcommand)]
enum SnapshotCommand {
    /// Introspect PostgreSQL and write portable metadata.
    Write(SnapshotArgs),
    /// Compare a snapshot to PostgreSQL without writing any files.
    Check(SnapshotArgs),
}

#[derive(Args)]
struct SnapshotArgs {
    /// Snapshot path to write or check.
    #[arg(long, default_value = "schema.json")]
    out: PathBuf,
    /// Schema to include. Repeat to select several schemas; defaults to public.
    #[arg(long, action = clap::ArgAction::Append)]
    schema: Vec<String>,
    /// Environment variable containing the PostgreSQL connection string.
    #[arg(long, default_value = "DATABASE_URL")]
    database_url_env: String,
    /// PostgreSQL connection string. Prefer an environment variable to avoid shell history.
    #[arg(long)]
    database_url: Option<String>,
}

#[expect(
    clippy::print_stderr,
    reason = "The CLI reports operational failures on stderr."
)]
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[expect(
    clippy::print_stdout,
    reason = "The standalone CLI reports results, not Cargo instructions."
)]
fn run(cli: Cli) -> Result<(), String> {
    let Command::Snapshot { command } = cli.command;
    match command {
        SnapshotCommand::Write(args) => {
            let snapshot = introspect(&args)?;
            snapshot.write_to(&args.out).map_err(|error| {
                format!("could not write snapshot {}: {error}", args.out.display())
            })?;
            println!("Wrote schema snapshot to {}", args.out.display());
        }
        SnapshotCommand::Check(args) => {
            // Read first: a missing or invalid baseline is an error even without credentials.
            let snapshot = read_snapshot(&args.out)?;
            let database = introspect(&args)?;
            let changes = schema_drift(&snapshot, &database)?;
            if !changes.is_empty() {
                return Err(format!(
                    "schema drift detected in {}:\n{}\nReview the database changes, then update the snapshot with \
                     `rp-supabase-codegen snapshot write` using the same --out and --schema options.",
                    args.out.display(),
                    changes.join("\n")
                ));
            }
            println!(
                "Schema snapshot {} matches the database",
                args.out.display()
            );
        }
    }
    Ok(())
}

fn introspect(args: &SnapshotArgs) -> Result<Snapshot, String> {
    let database_url = match &args.database_url {
        Some(url) => Cow::Borrowed(url.as_str()),
        None => Cow::Owned(std::env::var(&args.database_url_env).map_err(|_error| {
            format!(
                "set {} to a PostgreSQL connection string or pass --database-url",
                args.database_url_env
            )
        })?),
    };
    let generator = if args.schema.is_empty() {
        Generator::new()
    } else {
        Generator::new().schemas(args.schema.iter().cloned())
    };
    // Snapshot acquisition hides driver diagnostics and does not require Rust type mappings.
    generator
        .snapshot_from_database(&database_url)
        .map_err(|error| error.to_string())
}

fn read_snapshot(path: &Path) -> Result<Snapshot, String> {
    #[derive(serde::Deserialize)]
    struct VersionHeader {
        version: u32,
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read snapshot {}: {error}", path.display()))?;
    let header: VersionHeader = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid snapshot {}: {error}", path.display()))?;
    if header.version != SNAPSHOT_VERSION {
        return Err(format!(
            "snapshot version {} is unsupported; expected {SNAPSHOT_VERSION}; \
             regenerate {} with `rp-supabase-codegen snapshot write`",
            header.version,
            path.display()
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid snapshot {}: {error}", path.display()))
}

fn schema_drift(snapshot: &Snapshot, database: &Snapshot) -> Result<Vec<String>, String> {
    let snapshot = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
    let database = serde_json::to_value(database).map_err(|error| error.to_string())?;
    let mut changes = Vec::new();
    compare_values("snapshot", "", &snapshot, &database, &mut changes);
    Ok(changes)
}

fn compare_values(
    path: &str,
    field: &str,
    snapshot: &Value,
    database: &Value,
    changes: &mut Vec<String>,
) {
    if snapshot == database {
        return;
    }
    match (snapshot, database) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys: BTreeSet<_> = expected.keys().chain(actual.keys()).collect();
            for key in keys {
                compare_entries(
                    &format!("{path}.{key}"),
                    key,
                    expected.get(key),
                    actual.get(key),
                    changes,
                );
            }
        }
        (Value::Array(expected), Value::Array(actual))
            if matches!(
                field,
                "schemas"
                    | "enums"
                    | "composites"
                    | "tables"
                    | "functions"
                    | "columns"
                    | "computed_fields"
                    | "fields"
                    | "foreign_keys"
                    | "unique_keys"
            ) && (field == "unique_keys"
                || expected.iter().chain(actual).all(Value::is_object)) =>
        {
            let expected = indexed_values(field, expected);
            let actual = indexed_values(field, actual);
            let keys: BTreeSet<_> = expected.keys().chain(actual.keys()).collect();
            for key in keys {
                compare_entries(
                    &format!("{path}[{key:?}]"),
                    "",
                    expected.get(key).copied(),
                    actual.get(key).copied(),
                    changes,
                );
            }
            // Object listing order is incidental, but PostgreSQL column order is metadata.
            if matches!(field, "columns" | "fields") {
                let expected_order = ordered_names(snapshot);
                let actual_order = ordered_names(database);
                if expected_order != actual_order {
                    changes.push(format!(
                        "~ {path}.order: snapshot={expected_order:?}, database={actual_order:?}"
                    ));
                }
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            for index in 0..expected.len().max(actual.len()) {
                compare_entries(
                    &format!("{path}[{index}]"),
                    "",
                    expected.get(index),
                    actual.get(index),
                    changes,
                );
            }
        }
        _ if snapshot != database => changes.push(format!(
            "~ {path}: snapshot={snapshot}, database={database}"
        )),
        _ => {}
    }
}

fn compare_entries(
    path: &str,
    field: &str,
    snapshot: Option<&Value>,
    database: Option<&Value>,
    changes: &mut Vec<String>,
) {
    match (snapshot, database) {
        (Some(expected), Some(actual)) => compare_values(path, field, expected, actual, changes),
        (Some(expected), None) => {
            changes.push(format!("- {path}: removed from database ({expected})"));
        }
        (None, Some(actual)) => changes.push(format!("+ {path}: added to database ({actual})")),
        (None, None) => {}
    }
}

fn indexed_values<'value>(field: &str, values: &'value [Value]) -> BTreeMap<String, &'value Value> {
    values
        .iter()
        .map(|value| {
            let key = if field == "unique_keys" {
                value.to_string()
            } else {
                let name = value.get("name").and_then(Value::as_str).unwrap_or("");
                if field == "functions" {
                    // PostgreSQL permits overloads. Argument types, not argument names or
                    // default flags, identify each overload in the portable metadata.
                    let types: Vec<_> = value
                        .get("arguments")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|argument| argument.get("ty"))
                        .map(Value::to_string)
                        .collect();
                    format!("{name}({})", types.join(", "))
                } else {
                    name.to_owned()
                }
            };
            (key, value)
        })
        .collect()
}

fn ordered_names(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "Test fixtures and drift serialization must be valid."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "Fixtures contain the schemas, tables, columns, and functions accessed by these tests."
)]
mod tests {
    use super::schema_drift;
    use rp_supabase_codegen::model::{SNAPSHOT_VERSION, Snapshot};
    use serde_json::json;

    fn snapshot() -> Snapshot {
        serde_json::from_value(json!({
            "version": SNAPSHOT_VERSION,
            "schemas": [{
                "name": "public", "enums": [], "composites": [], "functions": [],
                "tables": [{
                    "name": "users", "kind": "table", "primary_key": ["id"],
                    "unique_keys": [["id"], ["email"]], "foreign_keys": [], "is_partition": false,
                    "computed_fields": [],
                    "columns": [
                        { "name": "id", "ty": { "kind": "builtin", "value": "int4" },
                          "nullable": false, "has_default": true, "generated": false, "identity": "none" },
                        { "name": "email", "ty": { "kind": "builtin", "value": "text" },
                          "nullable": false, "has_default": false, "generated": false, "identity": "none" }
                    ]
                }]
            }]
        })).unwrap()
    }

    #[test]
    fn drift_identifies_fields_and_added_objects() {
        let expected = snapshot();
        let mut actual = expected.clone();
        actual.schemas[0].tables[0].columns[1].nullable = true;
        let mut added = actual.schemas[0].tables[0].clone();
        added.name = "accounts".to_owned();
        actual.schemas[0].tables.push(added);
        let changes = schema_drift(&expected, &actual).unwrap();
        assert_eq!(changes.len(), 2);
        assert!(changes[0].starts_with("+ snapshot.schemas[\"public\"].tables[\"accounts\"]"));
        assert_eq!(
            changes[1],
            "~ snapshot.schemas[\"public\"].tables[\"users\"].columns[\"email\"].nullable: snapshot=false, database=true"
        );
    }
    #[test]
    fn computed_field_drift_is_keyed_by_name() {
        use rp_supabase_codegen::model::{ComputedField, PgType};
        let field = |name: &str| ComputedField {
            name: name.to_owned(),
            ty: PgType::Builtin("text".to_owned()),
        };
        let mut expected = snapshot();
        expected.schemas[0].tables[0].computed_fields = vec![field("label")];
        let mut actual = expected.clone();
        actual.schemas[0].tables[0].computed_fields = vec![field("display"), field("label")];
        let changes = schema_drift(&expected, &actual).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].starts_with(
            "+ snapshot.schemas[\"public\"].tables[\"users\"].computed_fields[\"display\"]:"
        ));
    }
    #[test]
    fn composite_foreign_key_drift_compares_every_source_column() {
        use rp_supabase_codegen::model::{ForeignKey, RelationRef};
        let mut expected = snapshot();
        let table = &mut expected.schemas[0].tables[0];
        let mut other_id = table.columns[0].clone();
        other_id.name = "other_id".to_owned();
        table.columns.push(other_id);
        table
            .unique_keys
            .push(vec!["id".to_owned(), "email".to_owned()]);
        table.foreign_keys.push(ForeignKey {
            name: "composite_fk".to_owned(),
            columns: vec!["id".to_owned(), "email".to_owned()],
            referenced_relation: RelationRef {
                schema: "public".to_owned(),
                name: "users".to_owned(),
            },
            referenced_columns: vec!["id".to_owned(), "email".to_owned()],
        });
        let mut actual = expected.clone();
        actual.schemas[0].tables[0].foreign_keys[0].columns[0] = "other_id".to_owned();
        assert_eq!(
            schema_drift(&expected, &actual).unwrap(),
            [
                "~ snapshot.schemas[\"public\"].tables[\"users\"].foreign_keys[\"composite_fk\"].columns[0]: snapshot=\"id\", database=\"other_id\""
            ]
        );
    }

    #[test]
    fn unordered_catalog_objects_and_constraints_do_not_drift() {
        let mut expected = snapshot();
        let mut other = expected.schemas[0].clone();
        other.name = "other".to_owned();
        expected.schemas.push(other);
        let mut actual = expected.clone();
        actual.schemas.reverse();
        for schema in &mut actual.schemas {
            schema.tables[0].unique_keys.reverse();
        }
        assert!(schema_drift(&expected, &actual).unwrap().is_empty());
    }

    #[test]
    fn column_order_is_portable_metadata() {
        let expected = snapshot();
        let mut actual = expected.clone();
        actual.schemas[0].tables[0].columns.reverse();
        let changes = schema_drift(&expected, &actual).unwrap();
        assert_eq!(
            changes,
            [
                "~ snapshot.schemas[\"public\"].tables[\"users\"].columns.order: snapshot=[\"id\", \"email\"], database=[\"email\", \"id\"]"
            ]
        );
    }

    #[test]
    fn overloaded_functions_compare_defaults_without_merging_overloads() {
        let mut value = serde_json::to_value(snapshot()).unwrap();
        value["schemas"][0]["functions"] = json!([
            { "name": "lookup", "arguments": [{"name": "id", "ty": {"kind": "builtin", "value": "int4"}, "has_default": false, "nullable": false}],
              "returns": {"kind": "type", "value": {"kind": "builtin", "value": "text"}}, "returns_set": false },
            { "name": "lookup", "arguments": [{"name": "id", "ty": {"kind": "builtin", "value": "text"}, "has_default": false, "nullable": false}],
              "returns": {"kind": "type", "value": {"kind": "builtin", "value": "text"}}, "returns_set": false }
        ]);
        let expected: Snapshot = serde_json::from_value(value).unwrap();
        let mut actual = expected.clone();
        actual.schemas[0].functions.reverse();
        assert!(schema_drift(&expected, &actual).unwrap().is_empty());
        actual.schemas[0].functions[0].arguments[0].has_default = true;
        let changes = schema_drift(&expected, &actual).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].contains("lookup("));
        assert!(changes[0].ends_with(".arguments[0].has_default: snapshot=false, database=true"));
    }

    #[test]
    fn drift_order_is_independent_of_catalog_listing_order() {
        let expected = snapshot();
        let mut actual = expected.clone();
        for name in ["zebra", "accounts"] {
            let mut table = actual.schemas[0].tables[0].clone();
            table.name = name.to_owned();
            actual.schemas[0].tables.push(table);
        }
        let changes = schema_drift(&expected, &actual).unwrap();
        actual.schemas[0].tables.reverse();
        assert_eq!(schema_drift(&expected, &actual).unwrap(), changes);
    }

    #[test]
    fn removed_objects_and_enum_variant_order_report_drift() {
        let mut expected = snapshot();
        expected.schemas[0]
            .enums
            .push(rp_supabase_codegen::model::Enum {
                name: "state".to_owned(),
                variants: vec!["pending".to_owned(), "active".to_owned()],
            });
        let mut actual = expected.clone();
        actual.schemas[0].tables.clear();
        actual.schemas[0].enums[0].variants.reverse();
        let changes = schema_drift(&expected, &actual).unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(
            changes[0],
            "~ snapshot.schemas[\"public\"].enums[\"state\"].variants[0]: snapshot=\"pending\", database=\"active\""
        );
        assert_eq!(
            changes[1],
            "~ snapshot.schemas[\"public\"].enums[\"state\"].variants[1]: snapshot=\"active\", database=\"pending\""
        );
        assert!(changes[2].starts_with(
            "- snapshot.schemas[\"public\"].tables[\"users\"]: removed from database"
        ));
    }
}
