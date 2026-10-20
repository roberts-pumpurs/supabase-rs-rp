//! Direct catalog extraction; never reads user rows or embeds connection diagnostics.

#![expect(
    clippy::min_ident_chars,
    reason = "Short closure bindings keep catalog joins and ordering expressions readable."
)]

use alloc::collections::{BTreeMap, BTreeSet};

use postgres::{IsolationLevel, Row, Transaction};
use postgres_native_tls::MakeTlsConnector;

use crate::Error;
use crate::model::{
    Argument, Column, Composite, ComputedField, Enum, ForeignKey, Function, Identity, PgType,
    RelationRef, ReturnType, SNAPSHOT_VERSION, Schema, Snapshot, Table, TableKind,
};

const CATALOG: &str = include_str!("catalog.sql");

// Driver and server diagnostics can contain passwords, connection strings, or SQL values.
// Deliberately retain only the operation, never the underlying error's Display/source.
fn database_error(operation: &str) -> Error {
    Error::Database(format!(
        "PostgreSQL {operation} failed; check connection settings, certificates, and catalog permissions"
    ))
}

fn query(
    tx: &mut Transaction<'_>,
    section: &str,
    schemas: Option<&[String]>,
) -> Result<Vec<Row>, Error> {
    let sql = CATALOG
        .split("-- query: ")
        .skip(1)
        .find_map(|part| {
            let (name, sql) = part.split_once('\n')?;
            (name == section).then_some(sql)
        })
        .ok_or_else(|| Error::Invalid(format!("missing catalog query {section}")))?;
    match schemas {
        Some(schemas) => tx.query(sql, &[&schemas]),
        None => tx.query(sql, &[]),
    }
    .map_err(|_error| database_error("catalog query"))
}

#[derive(Clone)]
struct TypeInfo {
    schema: String,
    name: String,
    kind: String,
    base: i64,
    element: i64,
    array: i64,
    relation: i64,
    not_null: bool,
    has_default: bool,
    comment: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Collation {
    Deterministic,
    Nondeterministic,
}

#[derive(Clone)]
struct Attribute {
    name: String,
    oid: i64,
    not_null: bool,
    has_default: bool,
    generated: bool,
    identity: Identity,
    collation: Collation,
}

struct Catalog {
    types: BTreeMap<i64, TypeInfo>,
    attributes: BTreeMap<i64, Vec<Attribute>>,
    enums: BTreeMap<i64, Vec<String>>,
    schemas: BTreeMap<String, Schema>,
    emitted: BTreeSet<i64>,
}

impl Catalog {
    fn schema(&mut self, name: &str) -> &mut Schema {
        self.schemas
            .entry(name.to_owned())
            .or_insert_with(|| Schema {
                name: name.to_owned(),
                enums: Vec::new(),
                composites: Vec::new(),
                tables: Vec::new(),
                functions: Vec::new(),
            })
    }

    fn info(&self, oid: i64) -> Result<&TypeInfo, Error> {
        self.types.get(&oid).ok_or_else(|| {
            Error::Invalid(format!(
                "catalog references missing PostgreSQL type OID {oid}"
            ))
        })
    }

    fn resolve(&mut self, oid: i64, domains: &mut BTreeSet<i64>) -> Result<PgType, Error> {
        let info = self.info(oid)?.clone();
        if info.kind == "d" {
            if !domains.insert(oid) {
                return Err(Error::Invalid(format!(
                    "cyclic domain {}.{}",
                    info.schema, info.name
                )));
            }
            let base = self.resolve(info.base, domains)?;
            domains.remove(&oid);
            return Ok(PgType::Domain {
                schema: info.schema,
                name: info.name,
                base: Box::new(base),
            });
        }
        // typelem alone also identifies non-array types such as int2vector. Verify
        // the inverse typarray relationship rather than guessing from type names.
        if info.element != 0 && self.info(info.element)?.array == oid {
            return Ok(PgType::Array(Box::new(
                self.resolve(info.element, domains)?,
            )));
        }
        if info.kind == "p" && info.name != "void" {
            return Err(Error::Invalid(format!(
                "unsupported PostgreSQL pseudotype {}.{}",
                info.schema, info.name
            )));
        }
        if info.kind == "e" && self.emitted.insert(oid) {
            let variants = self.enums.get(&oid).cloned().unwrap_or_default();
            self.schema(&info.schema).enums.push(Enum {
                name: info.name.clone(),
                variants,
            });
        }
        if info.kind == "c" && self.emitted.insert(oid) {
            let attributes = self
                .attributes
                .get(&info.relation)
                .cloned()
                .unwrap_or_default();
            let mut fields = Vec::with_capacity(attributes.len());
            for attribute in attributes {
                fields.push(self.column(&attribute, true)?);
            }
            let fields_annotation = crate::model::annotation(info.comment.as_deref(), "@not_null")?;
            crate::model::set_not_null(
                &mut fields,
                &fields_annotation,
                &format!("{}.{}", info.schema, info.name),
            )?;
            self.schema(&info.schema).composites.push(Composite {
                name: info.name.clone(),
                fields,
            });
        }
        if info.schema == "pg_catalog" && info.kind != "e" && info.kind != "c" {
            Ok(PgType::Builtin(info.name))
        } else {
            // Extension/base/range types preserve their identity for explicit overrides.
            Ok(PgType::Named {
                schema: info.schema,
                name: info.name,
            })
        }
    }

    fn column(&mut self, attribute: &Attribute, conservative: bool) -> Result<Column, Error> {
        let mut oid = attribute.oid;
        let mut not_null = attribute.not_null;
        let mut has_default = attribute.has_default;
        let mut visited = BTreeSet::new();
        loop {
            let info = self.info(oid)?;
            if info.kind != "d" {
                break;
            }
            if !visited.insert(oid) {
                return Err(Error::Invalid(format!(
                    "cyclic domain {}.{}",
                    info.schema, info.name
                )));
            }
            not_null |= info.not_null;
            has_default |= info.has_default;
            oid = info.base;
        }
        Ok(Column {
            name: attribute.name.clone(),
            ty: self.resolve(attribute.oid, &mut BTreeSet::new())?,
            nullable: conservative || !not_null,
            has_default: !conservative && has_default,
            generated: !conservative && attribute.generated,
            identity: if conservative {
                Identity::None
            } else {
                attribute.identity
            },
        })
    }

    fn is_pseudo(&self, oid: i64) -> Result<bool, Error> {
        let info = self.info(oid)?;
        if info.kind == "d" {
            return self.is_pseudo(info.base);
        }
        Ok(info.kind == "p" && info.name != "void")
    }
}

fn infer_view_nullability(
    catalog: &Catalog,
    row: &Row,
    base_relations: &BTreeSet<i64>,
    columns: &mut [Column],
) {
    let Some((relation, projected)) = row
        .get::<_, Option<&str>>("definition")
        .and_then(crate::model::view_columns)
    else {
        return;
    };
    let [namespace, name] = relation.as_slice() else {
        return;
    };
    let Some(base) = catalog
        .types
        .values()
        .find(|info| &info.schema == namespace && &info.name == name && info.kind == "c")
    else {
        return;
    };
    if !base_relations.contains(&base.relation) {
        return;
    }
    let Some(attributes) = catalog.attributes.get(&base.relation) else {
        return;
    };
    for (column, source) in columns.iter_mut().zip(projected) {
        if let Some(source) = source {
            column.nullable = !attributes
                .iter()
                .any(|attribute| attribute.name == source && attribute.not_null);
        }
    }
}

fn collect_check_enums(
    catalog: &Catalog,
    relation: &Row,
    constraint: &Row,
    columns: &[Column],
    check_enums: &mut BTreeMap<String, Vec<String>>,
) {
    let Some(attributes) = catalog.attributes.get(&relation.get::<_, i64>("oid")) else {
        return;
    };
    for column in columns {
        // bpchar equality ignores trailing spaces, so it cannot define exact enum labels.
        if !matches!(&column.ty, PgType::Builtin(name) if matches!(name.as_str(), "text" | "varchar"))
            || !attributes.iter().any(|attribute| {
                attribute.name == column.name && attribute.collation == Collation::Deterministic
            })
        {
            continue;
        }
        if let Some(variants) = constraint
            .get::<_, Option<&str>>("expression")
            .and_then(|expression| crate::model::check_values(expression, &column.name))
        {
            check_enums
                .entry(column.name.clone())
                .and_modify(|existing| {
                    existing.retain(|variant| variants.contains(variant));
                })
                .or_insert(variants);
        }
    }
}

fn load_catalog(tx: &mut Transaction<'_>, schemas: &[String]) -> Result<Catalog, Error> {
    let mut catalog = Catalog {
        types: BTreeMap::new(),
        attributes: BTreeMap::new(),
        enums: BTreeMap::new(),
        schemas: BTreeMap::new(),
        emitted: BTreeSet::new(),
    };
    for row in query(tx, "schemas", Some(schemas))? {
        catalog.schema(row.get::<_, &str>("name"));
    }
    for schema in schemas {
        if !catalog.schemas.contains_key(schema) {
            return Err(Error::Invalid(format!(
                "requested PostgreSQL schema {schema:?} does not exist"
            )));
        }
    }
    for row in query(tx, "types", None)? {
        catalog.types.insert(
            row.get("oid"),
            TypeInfo {
                schema: row.get("schema"),
                name: row.get("name"),
                kind: row.get("kind"),
                base: row.get("base"),
                element: row.get("element"),
                array: row.get("array"),
                relation: row.get("relation"),
                not_null: row.get("not_null"),
                has_default: row.get("has_default"),
                comment: row.get("comment"),
            },
        );
    }
    for row in query(tx, "enums", None)? {
        catalog
            .enums
            .entry(row.get("oid"))
            .or_default()
            .push(row.get("label"));
    }
    for row in query(tx, "attributes", None)? {
        let identity: &str = row.get("identity");
        catalog
            .attributes
            .entry(row.get("relation"))
            .or_default()
            .push(Attribute {
                name: row.get("name"),
                oid: row.get("type_oid"),
                not_null: row.get("not_null"),
                has_default: row.get("has_default"),
                generated: !row.get::<_, &str>("generated").is_empty(),
                identity: match identity {
                    "a" => Identity::Always,
                    "d" => Identity::ByDefault,
                    _ => Identity::None,
                },
                collation: if row.get("deterministic_collation") {
                    Collation::Deterministic
                } else {
                    Collation::Nondeterministic
                },
            });
    }
    // Include enums and composites in requested schemas, including nullable
    // relation-row composites, even if currently unreferenced.
    let roots: Vec<i64> = catalog
        .types
        .iter()
        .filter_map(|(&oid, info)| {
            (schemas.contains(&info.schema) && matches!(info.kind.as_str(), "e" | "c"))
                .then_some(oid)
        })
        .collect();
    for oid in roots {
        catalog.resolve(oid, &mut BTreeSet::new())?;
    }
    Ok(catalog)
}

// PostgREST resolves `relation.name` field notation to a function on the row type. Attach the
// supported subset: one input of a same-schema relation's row type and a single non-record,
// non-pseudotype result. `void` is excluded too: a row field renders it as `""`, not unit.
// A physical column with the same name shadows the function, so it is skipped.
#[expect(
    clippy::too_many_arguments,
    reason = "The caller already decoded these function facts from one catalog row."
)]
fn attach_computed_field(
    catalog: &mut Catalog,
    relations: &BTreeMap<i64, (String, usize)>,
    schema: &str,
    name: String,
    inputs: &[(String, i64, bool)],
    has_outputs: bool,
    returns_set: bool,
    return_oid: i64,
) -> Result<(), Error> {
    let [(_, input, _)] = inputs else {
        return Ok(());
    };
    let relation = relations
        .get(&catalog.info(*input)?.relation)
        .filter(|(relation_schema, _)| relation_schema == schema)
        .map(|&(_, index)| index);
    let Some(index) = relation else {
        return Ok(());
    };
    if returns_set || has_outputs || catalog.info(return_oid)?.kind == "p" {
        return Ok(());
    }
    let ty = catalog.resolve(return_oid, &mut BTreeSet::new())?;
    let table = catalog
        .schema(schema)
        .tables
        .get_mut(index)
        .ok_or_else(|| database_error("computed field relation"))?;
    if !table.columns.iter().any(|column| column.name == name) {
        table.computed_fields.push(ComputedField { name, ty });
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Catalog extraction remains one ordered read-only transaction over related metadata."
)]
pub fn introspect(url: &str, schemas: &[String]) -> Result<Snapshot, Error> {
    let mut config: postgres::Config = url
        .parse()
        .map_err(|_error| database_error("connection configuration"))?;
    // Never permit the driver's default Prefer mode to fall back to plaintext.
    // Explicit sslmode=disable remains available for local databases.
    if config.get_ssl_mode() == postgres::config::SslMode::Prefer {
        config.ssl_mode(postgres::config::SslMode::Require);
    }
    // native-tls defaults verify both the certificate chain and server hostname.
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|_error| database_error("TLS initialization"))?;
    let mut client = config
        .connect(MakeTlsConnector::new(tls))
        .map_err(|_error| database_error("connection"))?;
    let mut tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .map_err(|_error| database_error("snapshot transaction"))?;
    tx.batch_execute("SET LOCAL search_path = ''")
        .map_err(|_error| database_error("catalog search path"))?;
    let mut catalog = load_catalog(&mut tx, schemas)?;
    let mut constraints: BTreeMap<i64, Vec<Row>> = BTreeMap::new();
    for row in query(&mut tx, "constraints", Some(schemas))? {
        constraints
            .entry(row.get("relation"))
            .or_default()
            .push(row);
    }
    let relation_rows = query(&mut tx, "relations", Some(schemas))?;
    let base_relations: BTreeSet<i64> = relation_rows
        .iter()
        .filter(|row| matches!(row.get::<_, &str>("kind"), "r" | "p" | "f"))
        .map(|row| row.get("oid"))
        .collect();
    // Relation OID to its schema and position, for attaching computed fields.
    let mut relations: BTreeMap<i64, (String, usize)> = BTreeMap::new();
    for row in relation_rows {
        let schema: String = row.get("schema");
        let kind = match row.get::<_, &str>("kind") {
            "v" => TableKind::View,
            "m" => TableKind::MaterializedView,
            _ => TableKind::Table,
        };
        let attributes = catalog
            .attributes
            .get(&row.get::<_, i64>("oid"))
            .cloned()
            .unwrap_or_default();
        let mut columns = Vec::with_capacity(attributes.len());
        for attribute in attributes {
            columns.push(catalog.column(&attribute, kind != TableKind::Table)?);
        }
        if kind == TableKind::View {
            infer_view_nullability(&catalog, &row, &base_relations, &mut columns);
        }
        if kind != TableKind::Table {
            let annotation = crate::model::annotation(row.get("comment"), "@not_null")?;
            crate::model::set_not_null(
                &mut columns,
                &annotation,
                &format!("{schema}.{}", row.get::<_, &str>("name")),
            )?;
        }
        let mut primary_key = None;
        let mut unique_keys = Vec::new();
        let mut foreign_keys = Vec::new();
        let mut check_enums: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for constraint in constraints
            .remove(&row.get::<_, i64>("oid"))
            .unwrap_or_default()
        {
            match constraint.get::<_, &str>("kind") {
                "p" => primary_key = Some(constraint.get("columns")),
                "u" => unique_keys.push(constraint.get("columns")),
                "f" => foreign_keys.push(ForeignKey {
                    name: constraint.get("name"),
                    columns: constraint.get("columns"),
                    referenced_relation: RelationRef {
                        schema: constraint.get("referenced_schema"),
                        name: constraint.get("referenced_name"),
                    },
                    referenced_columns: constraint.get("referenced_columns"),
                }),
                "c" => collect_check_enums(&catalog, &row, &constraint, &columns, &mut check_enums),
                _ => return Err(database_error("unsupported constraint kind")),
            }
        }
        for (column_name, variants) in check_enums {
            let enum_name = format!("{}_{}", row.get::<_, &str>("name"), column_name);
            if variants.is_empty()
                || catalog
                    .schema(&schema)
                    .enums
                    .iter()
                    .any(|enumeration| enumeration.name == enum_name)
            {
                return Err(Error::Invalid(format!(
                    "invalid or colliding CHECK enum {schema}.{enum_name}"
                )));
            }
            catalog.schema(&schema).enums.push(Enum {
                name: enum_name.clone(),
                variants,
            });
            if let Some(column) = columns.iter_mut().find(|column| column.name == column_name) {
                column.ty = PgType::Named {
                    schema: schema.clone(),
                    name: enum_name,
                };
            }
        }
        let tables = &mut catalog.schema(&schema).tables;
        relations.insert(row.get("oid"), (schema.clone(), tables.len()));
        tables.push(Table {
            name: row.get("name"),
            kind,
            columns,
            primary_key,
            unique_keys,
            foreign_keys,
            is_partition: row.get("is_partition"),
            computed_fields: Vec::new(),
        });
    }
    for row in query(&mut tx, "functions", Some(schemas))? {
        let types: Vec<i64> = row.get("types");
        let modes: Option<Vec<String>> = row.get("modes");
        let names: Option<Vec<String>> = row.get("names");
        let input_count: i32 = row.get("input_count");
        let defaults: i32 = row.get("defaults");
        let return_oid: i64 = row.get("return_oid");
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut eligible = true;
        let mut input_index = 0_i32;
        for (index, &oid) in types.iter().enumerate() {
            let mode = modes
                .as_ref()
                .and_then(|m| m.get(index))
                .map_or("i", String::as_str);
            let name = names
                .as_ref()
                .and_then(|n| n.get(index))
                .cloned()
                .unwrap_or_default();
            if matches!(mode, "i" | "b" | "v") {
                if name.is_empty() || catalog.is_pseudo(oid)? {
                    eligible = false;
                }
                inputs.push((
                    name.clone(),
                    oid,
                    input_index >= input_count.saturating_sub(defaults),
                ));
                input_index = input_index.saturating_add(1_i32);
            }
            if matches!(mode, "o" | "b" | "t") {
                outputs.push((name, oid));
            }
        }
        // Named-object RPC bindings intentionally exclude unnamed inputs and dynamic/
        // polymorphic pseudotypes. OUT records are eligible only with known named fields.
        if catalog.is_pseudo(return_oid)?
            && (catalog.info(return_oid)?.name != "record" || outputs.is_empty())
        {
            eligible = false;
        }
        for (name, oid) in &outputs {
            if name.is_empty() || catalog.is_pseudo(*oid)? {
                eligible = false;
            }
        }
        let schema: String = row.get("schema");
        attach_computed_field(
            &mut catalog,
            &relations,
            &schema,
            row.get("name"),
            &inputs,
            !outputs.is_empty(),
            row.get("returns_set"),
            return_oid,
        )?;
        if !eligible {
            continue;
        }
        let mut arguments = Vec::with_capacity(inputs.len());
        for (name, oid, has_default) in inputs {
            arguments.push(Argument {
                name,
                ty: catalog.resolve(oid, &mut BTreeSet::new())?,
                has_default,
                nullable: false,
            });
        }
        let nullable = crate::model::annotation(row.get("comment"), "@nullable")?;
        for name in nullable {
            let argument = arguments
                .iter_mut()
                .find(|argument| argument.name == name)
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "unknown @nullable argument {}.{name}",
                        row.get::<_, &str>("name")
                    ))
                })?;
            argument.nullable = true;
        }
        let mut returns = if outputs.is_empty() {
            ReturnType::Type(catalog.resolve(return_oid, &mut BTreeSet::new())?)
        } else {
            let mut fields = Vec::with_capacity(outputs.len());
            for (name, oid) in outputs {
                fields.push(Column {
                    name,
                    ty: catalog.resolve(oid, &mut BTreeSet::new())?,
                    nullable: true,
                    has_default: false,
                    generated: false,
                    identity: Identity::None,
                });
            }
            ReturnType::Record(fields)
        };
        let annotation = crate::model::annotation(row.get("comment"), "@not_null")?;
        if !annotation.is_empty() {
            if let ReturnType::Record(fields) = &mut returns {
                crate::model::set_not_null(fields, &annotation, row.get("name"))?;
            } else {
                return Err(Error::Invalid(
                    "@not_null requires an RPC record return".into(),
                ));
            }
        }
        catalog.schema(&schema).functions.push(Function {
            name: row.get("name"),
            arguments,
            returns,
            returns_set: row.get("returns_set"),
        });
    }
    tx.commit()
        .map_err(|_error| database_error("snapshot commit"))?;
    for schema in catalog.schemas.values_mut() {
        schema.enums.sort_by(|a, b| a.name.cmp(&b.name));
        schema.composites.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Ok(Snapshot {
        version: SNAPSHOT_VERSION,
        schemas: catalog.schemas.into_values().collect(),
    })
}
