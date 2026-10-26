#![expect(
    clippy::min_ident_chars,
    reason = "Short closure bindings keep nested metadata traversals and quoted Rust readable."
)]

use alloc::collections::{BTreeMap, BTreeSet};

use heck::{ToSnakeCase as _, ToUpperCamelCase as _};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::parse::Parser as _;

use crate::{Config, Error, model::*};

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn selection_identifier(name: &str) -> String {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && !matches!(
            name,
            "select" | "columns" | "on_conflict" | "order" | "limit" | "offset" | "and" | "or"
        )
    {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

pub fn ident(name: &str, camel: bool) -> Result<Ident, Error> {
    let mut normalized = if camel {
        name.to_upper_camel_case()
    } else {
        name.to_snake_case()
    };
    if normalized.is_empty() {
        return Err(invalid(format!(
            "identifier {name:?} has no Rust identifier characters"
        )));
    }
    if normalized.starts_with(|c: char| c.is_ascii_digit()) {
        normalized.insert(0, '_');
    }
    if matches!(normalized.as_str(), "self" | "Self" | "super" | "crate") {
        normalized.push('_');
    }
    syn::parse_str::<Ident>(&normalized)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{normalized}")))
        .map_err(|_error| {
            invalid(format!(
                "cannot convert PostgreSQL identifier {name:?} to Rust"
            ))
        })
}

fn key_name(name: &Ident, runtime: &syn::Path) -> TokenStream {
    let name = name.to_string();
    let characters = name.strip_prefix("r#").unwrap_or(&name).chars();
    let characters = characters.map(|character| quote!(#runtime::Character<#character>));
    quote!((#(#characters,)*))
}

fn column_lookup_diagnostic(schema: &str, table: &Table) -> Result<(String, String), Error> {
    let names = table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .chain(
            table
                .computed_fields
                .iter()
                .map(|field| field.name.as_str()),
        )
        .map(|name| ident(name, false).map(|name| name.to_string()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    // Diagnostic attributes interpret braces even inside literal database names.
    let escape = |value: &str| value.replace('{', "{{").replace('}', "}}");
    let relation = escape(&format!("{schema}.{}", table.name));
    let valid = if names.is_empty() {
        "valid columns: (none)".to_owned()
    } else {
        format!(
            "valid columns: {}",
            escape(&names.into_iter().collect::<Vec<_>>().join(", "))
        )
    };
    Ok((format!("no column `{{Name}}` on table `{relation}`"), valid))
}

fn column_lookup(schema: &str, table: &Table, runtime: &syn::Path) -> Result<TokenStream, Error> {
    let (message, note) = column_lookup_diagnostic(schema, table)?;
    // Resolve the configured path before Name/Selected can shadow its first segment.
    let mut alias = "__supabase_column_runtime".to_owned();
    while runtime
        .segments
        .iter()
        .any(|segment| segment.ident == alias)
    {
        alias.push('_');
    }
    let alias = Ident::new(&alias, Span::call_site());
    let runtime_path = runtime;
    let runtime = &alias;
    Ok(quote!(
        use #runtime_path as #runtime;
        #[doc(hidden)]
        #[diagnostic::on_unimplemented(
            message = #message,
            label = "unknown column",
            note = #note
        )]
        pub trait SelectedColumn<Name>: #runtime::Relation {
            type Column: #runtime::Column<Relation = Self>;
        }
        #[diagnostic::do_not_recommend]
        impl<Name: #runtime::KeyName> SelectedColumn<Name> for Row
        where
            Self: #runtime::ColumnByKey<Name::Key>,
        {
            type Column = <Self as #runtime::ColumnByKey<Name::Key>>::Column;
        }
        #[doc(hidden)]
        #[derive(::core::marker::Copy, ::core::clone::Clone, ::core::default::Default)]
        pub struct ColumnLookup;
        impl #runtime::selection::ColumnLookup for ColumnLookup {
            type Relation = Row;
        }
        impl ColumnLookup {
            #[must_use]
            #[allow(
                clippy::unused_self,
                reason = "The receiver selects table-local diagnostics through inherent method dispatch."
            )]
            pub fn column<Name, Selected>(self, _: Name) -> #runtime::selection::Scalar<Selected>
            where
                Row: SelectedColumn<Name, Column = Selected>,
            {
                #runtime::selection::Scalar::<Selected>::new()
            }
        }
    ))
}

fn unique<'a>(
    names: impl IntoIterator<Item = &'a str>,
    camel: bool,
    context: &str,
) -> Result<(), Error> {
    let mut seen = BTreeMap::new();
    for name in names {
        let rust = ident(name, camel)?.to_string();
        if let Some(previous) = seen.insert(rust.clone(), name) {
            return Err(invalid(format!(
                "{context}: identifiers {previous:?} and {name:?} collide as {rust}"
            )));
        }
    }
    Ok(())
}

fn key_columns<'a>(
    columns: &'a [String],
    table: &Table,
    context: &str,
) -> Result<BTreeSet<&'a str>, Error> {
    let names: BTreeSet<_> = columns.iter().map(String::as_str).collect();
    if names.is_empty()
        || names.len() != columns.len()
        || names
            .iter()
            .any(|name| !table.columns.iter().any(|column| column.name == *name))
    {
        return Err(invalid(format!(
            "{context}: empty, duplicate, or unknown key columns"
        )));
    }
    Ok(names)
}

fn validate_relationships(snapshot: &Snapshot) -> Result<(), Error> {
    for schema in &snapshot.schemas {
        for table in &schema.tables {
            let context = format!("{}.{}", schema.name, table.name);
            if let Some(key) = &table.primary_key {
                key_columns(key, table, &context)?;
                if key.iter().any(|name| {
                    table
                        .columns
                        .iter()
                        .any(|column| column.name == *name && column.nullable)
                }) {
                    return Err(invalid(format!(
                        "{context}: primary key columns cannot be nullable"
                    )));
                }
            }
            for key in &table.unique_keys {
                key_columns(key, table, &context)?;
            }
            let mut names = BTreeSet::new();
            for foreign_key in &table.foreign_keys {
                let context = format!("{context} foreign key {:?}", foreign_key.name);
                if foreign_key.name.is_empty() || !names.insert(&foreign_key.name) {
                    return Err(invalid(format!(
                        "{context}: empty or duplicate constraint name"
                    )));
                }
                key_columns(&foreign_key.columns, table, &context)?;
                if foreign_key.columns.len() != foreign_key.referenced_columns.len()
                    || foreign_key.referenced_relation.schema.is_empty()
                    || foreign_key.referenced_relation.name.is_empty()
                    || foreign_key.referenced_columns.iter().any(String::is_empty)
                    || foreign_key
                        .referenced_columns
                        .iter()
                        .collect::<BTreeSet<_>>()
                        .len()
                        != foreign_key.referenced_columns.len()
                {
                    return Err(invalid(format!("{context}: invalid referenced key")));
                }
                let target = snapshot
                    .schemas
                    .iter()
                    .find(|schema| schema.name == foreign_key.referenced_relation.schema)
                    .and_then(|schema| {
                        schema
                            .tables
                            .iter()
                            .find(|table| table.name == foreign_key.referenced_relation.name)
                    });
                if let Some(target) = target {
                    // A referenced unique index need not be represented by a UNIQUE constraint.
                    key_columns(&foreign_key.referenced_columns, target, &context)?;
                }
            }
        }
    }
    Ok(())
}

type RelationshipEdge<'a> = (String, &'a Table, &'a ForeignKey, bool);

fn relationship_edges<'a>(schema: &'a Schema, table: &'a Table) -> Vec<RelationshipEdge<'a>> {
    let mut edges = Vec::new();
    if table.kind == TableKind::Table && !table.is_partition {
        for source in &schema.tables {
            if source.kind != TableKind::Table || source.is_partition || source.name == table.name {
                continue;
            }
            for foreign_key in &source.foreign_keys {
                if foreign_key.referenced_relation.schema != schema.name {
                    continue;
                }
                if foreign_key.referenced_relation.name == table.name {
                    let columns = foreign_key.columns.iter().collect::<BTreeSet<_>>();
                    let to_one = source
                        .primary_key
                        .iter()
                        .chain(source.unique_keys.iter())
                        .any(|key| key.iter().collect::<BTreeSet<_>>() == columns);
                    edges.push((
                        format!(
                            "{}_{}",
                            source.name.to_snake_case(),
                            foreign_key.name.to_snake_case()
                        ),
                        source,
                        foreign_key,
                        to_one,
                    ));
                }
            }
        }
        for foreign_key in &table.foreign_keys {
            if foreign_key.referenced_relation.schema != schema.name
                || foreign_key.referenced_relation.name == table.name
            {
                continue;
            }
            if let Some(target) = schema.tables.iter().find(|target| {
                target.name == foreign_key.referenced_relation.name
                    && target.kind == TableKind::Table
                    && !target.is_partition
            }) {
                edges.push((foreign_key.name.clone(), target, foreign_key, true));
            }
        }
    }
    edges.sort_by(|left, right| left.0.cmp(&right.0));
    edges
}

fn relationship_aliases(
    schema: &Schema,
    table: &Table,
    edges: &[RelationshipEdge<'_>],
    config: &Config,
    used_aliases: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<Ident>>, Error> {
    let mut alias_candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, target, foreign_key, _) in edges {
        let alias = if table
            .foreign_keys
            .iter()
            .any(|key| core::ptr::eq(key, *foreign_key))
        {
            match foreign_key.columns.as_slice() {
                [column] => Some(
                    column
                        .strip_suffix("_id")
                        .filter(|stem| !stem.is_empty())
                        .unwrap_or(column)
                        .to_owned(),
                ),
                _ => None,
            }
        } else {
            Some(target.name.clone())
        };
        if let Some(alias) = alias {
            alias_candidates
                .entry(ident(&alias, false)?.to_string())
                .or_default()
                .push(name.clone());
        }
    }
    let mut occupied: BTreeSet<String> = edges
        .iter()
        .map(|edge| ident(&edge.0, false).map(|name| name.to_string()))
        .collect::<Result<_, _>>()?;
    let mut aliases: BTreeMap<String, Vec<Ident>> = BTreeMap::new();
    for (alias, names) in alias_candidates {
        if names.len() == 1
            && !occupied.contains(&alias)
            && let Some(name) = names.first()
        {
            aliases.entry(name.clone()).or_default().push(
                syn::parse_str(&alias)
                    .map_err(|error| invalid(format!("invalid relationship alias: {error}")))?,
            );
            occupied.insert(alias);
        }
    }
    for (name, _, _, _) in edges {
        let target = format!(
            "{}.tables.{}.relationships.{}",
            ident(&schema.name, false)?,
            ident(&table.name, false)?,
            ident(name, false)?
        );
        if let Some(alias) = config.relationship_aliases.get(&target) {
            let alias = syn::parse_str::<Ident>(alias).map_err(|error| {
                invalid(format!("invalid relationship alias for {target}: {error}"))
            })?;
            if occupied.iter().any(|name| {
                name.trim_start_matches("r#") == alias.to_string().trim_start_matches("r#")
            }) {
                return Err(invalid(format!(
                    "relationship alias collision {target}: {alias}"
                )));
            }
            occupied.insert(alias.to_string());
            used_aliases.insert(target);
            aliases.entry(name.clone()).or_default().push(alias);
        }
    }
    Ok(aliases)
}

fn relationship_markers(
    schema: &Schema,
    table: &Table,
    runtime: &syn::Path,
    config: &Config,
    used_aliases: &mut BTreeSet<String>,
) -> Result<TokenStream, Error> {
    let edges = relationship_edges(schema, table);
    unique(
        edges.iter().map(|edge| edge.0.as_str()),
        false,
        &format!("{}.{}.relationships", schema.name, table.name),
    )?;
    let mut aliases = relationship_aliases(schema, table, &edges, config, used_aliases)?;
    let mut output = TokenStream::new();
    for (name, target, foreign_key, to_one) in edges {
        let edge_aliases = aliases.remove(&name).unwrap_or_default();
        let name = ident(&name, false)?;
        let key = key_name(&name, runtime);
        let target_name = ident(&target.name, false)?;
        let resource = selection_identifier(&target.name);
        let hint = selection_identifier(&foreign_key.name);
        let cardinality = if to_one {
            quote!(#runtime::ToOne)
        } else {
            quote!(#runtime::ToMany)
        };
        output.extend(quote!(
            #[allow(non_camel_case_types)]
            #[derive(::core::marker::Copy, ::core::clone::Clone)]
            pub struct #name;
            impl #runtime::Relationship for #name {
                type Source = super::Row;
                type Target = super::super::#target_name::Row;
                type Cardinality = #cardinality;
                const RESOURCE: &'static ::core::primitive::str = #resource;
                const HINT: &'static ::core::primitive::str = #hint;
            }
            impl #runtime::RelationshipByKey<#key> for super::Row {
                type Edge = #name;
            }
        ));
        for alias in edge_aliases {
            let key = key_name(&alias, runtime);
            output.extend(quote!(
                #[allow(unused_imports)]
                pub use self::#name as #alias;
                impl #runtime::RelationshipByKey<#key> for super::Row {
                    type Edge = #name;
                }
            ));
        }
    }
    Ok(output)
}

fn identifiers(tokens: TokenStream, names: &mut BTreeSet<String>) {
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Ident(name) => {
                names.insert(name.to_string());
            }
            proc_macro2::TokenTree::Group(group) => identifiers(group.stream(), names),
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
}

// Bind override syntax in the prelude's scope, before nested generated names can shadow it.
fn override_aliases(
    prelude: &syn::File,
    types: BTreeMap<String, syn::Type>,
) -> (TokenStream, BTreeMap<String, Ident>) {
    let mut occupied = BTreeSet::new();
    identifiers(quote!(#prelude), &mut occupied);
    for ty in types.values() {
        identifiers(quote!(#ty), &mut occupied);
    }
    let mut aliases = TokenStream::new();
    let mut overrides = BTreeMap::new();
    let mut serial = 0_usize;
    for (key, ty) in types {
        let name = loop {
            let name = format!("__SupabaseCodegenType{serial}");
            serial = serial.saturating_add(1_usize);
            if occupied.insert(name.clone()) {
                break Ident::new(&name, Span::call_site());
            }
        };
        aliases.extend(quote!(type #name = #ty;));
        overrides.insert(key, name);
    }
    (aliases, overrides)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldPresence {
    Excluded,
    Required,
    Omittable,
}

#[derive(Clone, Copy)]
enum FieldMode {
    Row,
    Insert,
    Update,
}

impl FieldMode {
    const fn presence(self, column: &ColumnContract<'_>) -> FieldPresence {
        match self {
            Self::Row => FieldPresence::Required,
            Self::Insert => column.insert,
            Self::Update => match column.insert {
                FieldPresence::Excluded => FieldPresence::Excluded,
                FieldPresence::Required | FieldPresence::Omittable => FieldPresence::Omittable,
            },
        }
    }
}

// Resolved once per relation; every rendering consumes the same read/write obligations.
#[derive(Clone, Copy)]
struct ColumnContract<'a> {
    column: &'a Column,
    nullable: bool,
    insert: FieldPresence,
}

impl<'a> ColumnContract<'a> {
    fn resolve(
        columns: &'a [Column],
        table_kind: Option<TableKind>,
        context: &str,
    ) -> Result<impl Iterator<Item = Self> + 'a, Error> {
        unique(
            columns.iter().map(|column| column.name.as_str()),
            false,
            context,
        )?;
        let base_table = table_kind == Some(TableKind::Table);
        Ok(columns.iter().map(move |column| {
            let nullable = column.nullable;
            let insert = if !base_table || column.generated || column.identity == Identity::Always {
                FieldPresence::Excluded
            } else if nullable || column.has_default || column.identity == Identity::ByDefault {
                FieldPresence::Omittable
            } else {
                FieldPresence::Required
            };
            Self {
                column,
                nullable,
                insert,
            }
        }))
    }

    fn value_type(&self, base: TokenStream) -> TokenStream {
        if self.nullable {
            quote!(::std::option::Option<#base>)
        } else {
            base
        }
    }
}

struct Emitter<'a> {
    config: &'a Config,
    runtime: syn::Path,
    derives: Vec<syn::Path>,
    attributes: Vec<syn::Attribute>,
    targeted: BTreeMap<String, Vec<syn::Attribute>>,
    used_targets: BTreeSet<String>,
    overrides: BTreeMap<String, Ident>,
    named: BTreeMap<(String, String), bool>, // true for composite, false for enum
    field_overrides: BTreeMap<String, Ident>,
    json_targets: BTreeSet<String>,
}

fn attributes(values: &[String]) -> Result<Vec<syn::Attribute>, Error> {
    let mut result = Vec::new();
    for value in values {
        let parsed = syn::Attribute::parse_outer
            .parse_str(value)
            .map_err(|e| invalid(format!("invalid type attribute {value:?}: {e}")))?;
        if parsed.is_empty() {
            return Err(invalid("type attribute cannot be empty"));
        }
        result.extend(parsed);
    }
    Ok(result)
}

impl Emitter<'_> {
    fn decoration(&mut self, target: &str, deserialize: bool, default: bool) -> TokenStream {
        self.used_targets.insert(target.to_owned());
        let attrs = &self.attributes;
        let targeted = self.targeted.get(target).cloned().unwrap_or_default();
        let mut derives: Vec<syn::Path> = vec![
            syn::parse_quote!(::serde::Serialize),
            syn::parse_quote!(::core::fmt::Debug),
            syn::parse_quote!(::core::clone::Clone),
        ];
        if deserialize {
            derives.push(syn::parse_quote!(::serde::Deserialize));
        }
        if default {
            derives.push(syn::parse_quote!(::core::default::Default));
        }
        for derive in &self.derives {
            let derive = match derive.get_ident().map(Ident::to_string).as_deref() {
                Some("Debug") => syn::parse_quote!(::core::fmt::Debug),
                Some("Clone") => syn::parse_quote!(::core::clone::Clone),
                Some("Default") => syn::parse_quote!(::core::default::Default),
                _ => derive.clone(),
            };
            if !derives
                .iter()
                .any(|p| quote!(#p).to_string() == quote!(#derive).to_string())
            {
                derives.push(derive.clone());
            }
        }
        quote!(
            #(#attrs)* #(#targeted)*
            #[allow(
                clippy::derive_partial_eq_without_eq,
                reason = "The caller selects derives; SQL fields and custom overrides may not implement Eq"
            )]
            #[derive(#(#derives),*)]
        )
    }

    fn ty(&self, ty: &PgType, depth: usize) -> Result<TokenStream, Error> {
        if let Some(overridden) = type_key(ty).and_then(|key| self.overrides.get(&key)) {
            let parents = core::iter::repeat_with(|| quote!(super::)).take(depth);
            return Ok(quote!(#(#parents)* #overridden));
        }
        match ty {
            PgType::Domain { base, .. } => self.ty(base, depth),
            PgType::Array(inner) => {
                let inner = self.ty(inner, depth)?;
                let runtime = &self.runtime;
                Ok(quote!(#runtime::Array<#inner>))
            }
            PgType::Named { schema, name } => {
                let composite = self.named.get(&(schema.clone(), name.clone())).ok_or_else(|| invalid(format!("unresolved PostgreSQL type {schema}.{name}; include its schema metadata or supply type_override")))?;
                let schema = ident(schema, false)?;
                let name = ident(name, true)?;
                let scope = if *composite {
                    quote!(composites)
                } else {
                    quote!(enums)
                };
                let parents = core::iter::repeat_with(|| quote!(super::)).take(depth);
                Ok(quote!(#(#parents)* #schema::#scope::#name))
            }
            PgType::Builtin(name) => {
                let name = name.strip_prefix("pg_catalog.").unwrap_or(name);
                Ok(match name {
                    "bool" | "boolean" => quote!(::core::primitive::bool),
                    "int2" | "smallint" => quote!(::core::primitive::i16),
                    "int4" | "integer" => quote!(::core::primitive::i32),
                    "int8" | "bigint" => quote!(::core::primitive::i64),
                    "oid" | "xid" | "cid" => quote!(::core::primitive::u32),
                    "float4" | "real" => quote!(::core::primitive::f32),
                    "float8" | "double precision" => quote!(::core::primitive::f64),
                    "numeric" | "decimal" => quote!(::serde_json::Number),
                    "json" | "jsonb" => quote!(::serde_json::Value),
                    "uuid" => quote!(::uuid::Uuid),
                    "date" => quote!(::chrono::NaiveDate),
                    "time" | "time without time zone" => quote!(::chrono::NaiveTime),
                    "timestamp" | "timestamp without time zone" => quote!(::chrono::NaiveDateTime),
                    "timestamptz" | "timestamp with time zone" => {
                        quote!(::chrono::DateTime<::chrono::FixedOffset>)
                    }
                    "void" => quote!(()),
                    "text"
                    | "varchar"
                    | "character varying"
                    | "bpchar"
                    | "character"
                    | "char"
                    | "name"
                    | "bytea"
                    | "inet"
                    | "cidr"
                    | "macaddr"
                    | "macaddr8"
                    | "interval"
                    | "timetz"
                    | "time with time zone"
                    | "point"
                    | "line"
                    | "lseg"
                    | "box"
                    | "path"
                    | "polygon"
                    | "circle"
                    | "int4range"
                    | "int8range"
                    | "numrange"
                    | "tsrange"
                    | "tstzrange"
                    | "daterange"
                    | "int4multirange"
                    | "int8multirange"
                    | "nummultirange"
                    | "tsmultirange"
                    | "tstzmultirange"
                    | "datemultirange"
                    | "tsvector"
                    | "tsquery"
                    | "bit"
                    | "varbit"
                    | "xml"
                    | "pg_lsn"
                    | "money" => quote!(::std::string::String),
                    _ => {
                        return Err(invalid(format!(
                            "unsupported PostgreSQL type pg_catalog.{name}; supply an explicit type_override"
                        )));
                    }
                })
            }
        }
    }

    fn field_ty(&self, ty: &PgType, target: &str, depth: usize) -> Result<TokenStream, Error> {
        self.field_overrides.get(target).map_or_else(
            || self.ty(ty, depth),
            |alias| {
                let parents = core::iter::repeat_with(|| quote!(super::)).take(depth);
                let base = quote!(#(#parents)* #alias);
                if self.json_targets.contains(target) {
                    Ok(self.json_wrappers(ty, base))
                } else {
                    Ok(base)
                }
            },
        )
    }

    // PostgREST stores a JSON null in a write payload as SQL NULL, so a non-null JSON write
    // field uses `NonNullJson` wherever the read side would decode `serde_json::Value`.
    fn write_field_ty(
        &self,
        ty: &PgType,
        target: &str,
        depth: usize,
    ) -> Result<TokenStream, Error> {
        if !self.field_overrides.contains_key(target) && self.is_json_value(ty) {
            let runtime = &self.runtime;
            return Ok(quote!(#runtime::NonNullJson));
        }
        self.field_ty(ty, target, depth)
    }

    // Whether `ty` renders as `serde_json::Value`, honoring type overrides like `ty` does.
    fn is_json_value(&self, ty: &PgType) -> bool {
        if type_key(ty).is_some_and(|key| self.overrides.contains_key(&key)) {
            return false;
        }
        match ty {
            PgType::Domain { base, .. } => self.is_json_value(base),
            PgType::Builtin(_) => is_json_column_type(ty),
            PgType::Named { .. } | PgType::Array(_) => false,
        }
    }

    fn json_wrappers(&self, ty: &PgType, base: TokenStream) -> TokenStream {
        match ty {
            PgType::Array(inner) => {
                let inner = self.json_wrappers(inner, base);
                let runtime = &self.runtime;
                quote!(#runtime::Array<#inner>)
            }
            PgType::Domain { base: inner, .. } => self.json_wrappers(inner, base),
            PgType::Builtin(_) | PgType::Named { .. } => base,
        }
    }

    fn fields<'a>(
        &self,
        columns: impl IntoIterator<Item = ColumnContract<'a>>,
        depth: usize,
        mode: FieldMode,
        target: &str,
    ) -> Result<TokenStream, Error> {
        let mut fields = TokenStream::new();
        for contract in columns {
            let presence = mode.presence(&contract);
            if presence == FieldPresence::Excluded {
                continue;
            }
            let column = contract.column;
            let name = ident(&column.name, false)?;
            let wire = &column.name;
            let field_target = format!("{target}.{}", column.name);
            let base = match mode {
                FieldMode::Insert | FieldMode::Update if !contract.nullable => {
                    self.write_field_ty(&column.ty, &field_target, depth)?
                }
                FieldMode::Row | FieldMode::Insert | FieldMode::Update => {
                    self.field_ty(&column.ty, &field_target, depth)?
                }
            };
            let ty = contract.value_type(base);
            if presence == FieldPresence::Omittable {
                let runtime = &self.runtime;
                let skip = format!(
                    "{}::Field::is_omit",
                    quote!(#runtime).to_string().replace(' ', "")
                );
                fields.extend(quote!(#[serde(rename = #wire, skip_serializing_if = #skip)] pub #name: #runtime::Field<#ty>,));
            } else {
                fields.extend(quote!(#[serde(rename = #wire)] pub #name: #ty,));
            }
        }
        Ok(fields)
    }

    fn is_record(&self, ty: &PgType) -> bool {
        match ty {
            PgType::Named { schema, name } => self
                .named
                .get(&(schema.clone(), name.clone()))
                .copied()
                .unwrap_or(false),
            PgType::Domain { base, .. } => self.is_record(base),
            PgType::Builtin(_) | PgType::Array(_) => false,
        }
    }

    fn function_returns(
        &mut self,
        function: &Function,
        target: &str,
    ) -> Result<(TokenStream, TokenStream), Error> {
        let mut record = TokenStream::new();
        let returns = match &function.returns {
            ReturnType::Record(columns) => {
                let decoration = self.decoration(&format!("{target}.Record"), true, false);
                let columns = ColumnContract::resolve(columns, None, target)?;
                let fields =
                    self.fields(columns, 3, FieldMode::Row, &format!("{target}.Record"))?;
                record.extend(quote!(#decoration pub struct Record { #fields }));
                if function.returns_set {
                    quote!(::std::vec::Vec<Record>)
                } else {
                    quote!(Record)
                }
            }
            ReturnType::Type(ty) => {
                let base = self.field_ty(ty, &format!("{target}.Returns"), 3)?;
                if matches!(ty, PgType::Builtin(name) if name == "void" || name == "pg_catalog.void")
                {
                    quote!(())
                } else if self.is_record(ty) {
                    if function.returns_set {
                        quote!(::std::vec::Vec<#base>)
                    } else {
                        base
                    }
                } else if function.returns_set {
                    quote!(::std::vec::Vec<::std::option::Option<#base>>)
                } else {
                    quote!(::std::option::Option<#base>)
                }
            }
        };
        Ok((record, returns))
    }
}

// Override lookup key: `schema.name` for named types, `pg_catalog.name` for unqualified builtins.
fn type_key(ty: &PgType) -> Option<String> {
    match ty {
        PgType::Builtin(name) if name.contains('.') => Some(name.clone()),
        PgType::Builtin(name) => Some(format!("pg_catalog.{name}")),
        PgType::Named { schema, name } | PgType::Domain { schema, name, .. } => {
            Some(format!("{schema}.{name}"))
        }
        PgType::Array(_) => None,
    }
}

fn is_json_column_type(ty: &PgType) -> bool {
    match ty {
        PgType::Builtin(name) => {
            matches!(
                name.strip_prefix("pg_catalog.").unwrap_or(name),
                "json" | "jsonb"
            )
        }
        PgType::Domain { base, .. } => is_json_column_type(base),
        PgType::Named { .. } | PgType::Array(_) => false,
    }
}

fn is_json_type(ty: &PgType) -> bool {
    match ty {
        PgType::Array(inner) | PgType::Domain { base: inner, .. } => is_json_type(inner),
        PgType::Builtin(_) | PgType::Named { .. } => is_json_column_type(ty),
    }
}

fn json_field_types<'a>(
    snapshot: &'a Snapshot,
    config: &Config,
) -> Result<BTreeMap<String, &'a PgType>, Error> {
    let mut fields = BTreeMap::new();
    for schema in &snapshot.schemas {
        let schema_ident = ident(&schema.name, false)?;
        for composite in &schema.composites {
            let name = ident(&composite.name, true)?;
            for column in &composite.fields {
                fields.insert(
                    format!("{schema_ident}.composites.{name}.{}", column.name),
                    &column.ty,
                );
            }
        }
        if !config.schemas.contains(&schema.name) {
            continue;
        }
        for table in &schema.tables {
            let name = ident(&table.name, false)?;
            let computed = table
                .computed_fields
                .iter()
                .map(|field| (&field.name, &field.ty));
            for (field, ty) in table
                .columns
                .iter()
                .map(|column| (&column.name, &column.ty))
                .chain(computed)
            {
                fields.insert(format!("{schema_ident}.tables.{name}.{field}"), ty);
            }
        }
        let mut groups: BTreeMap<&str, Vec<&Function>> = BTreeMap::new();
        for function in &schema.functions {
            groups.entry(&function.name).or_default().push(function);
        }
        for (name, mut functions) in groups {
            functions.sort_by_key(|function| {
                function
                    .arguments
                    .iter()
                    .map(|argument| format!("{}:{:?}", argument.name, argument.ty))
                    .collect::<Vec<_>>()
            });
            let count = functions.len();
            let base = ident(name, false)?.to_string();
            for (index, function) in functions.into_iter().enumerate() {
                let name = if count == 1 {
                    base.clone()
                } else {
                    format!("{}_{index}", base.strip_prefix("r#").unwrap_or(&base))
                };
                let target = format!("{schema_ident}.functions.{name}");
                for argument in &function.arguments {
                    fields.insert(format!("{target}.Args.{}", argument.name), &argument.ty);
                }
                match &function.returns {
                    ReturnType::Type(ty) => {
                        fields.insert(format!("{target}.Returns"), ty);
                    }
                    ReturnType::Record(columns) => {
                        for column in columns {
                            fields.insert(format!("{target}.Record.{}", column.name), &column.ty);
                        }
                    }
                }
            }
        }
    }
    Ok(fields)
}

fn references(ty: &PgType, config: &Config, output: &mut BTreeSet<String>) {
    match ty {
        PgType::Named { schema, name } => {
            if !config
                .type_overrides
                .contains_key(&format!("{schema}.{name}"))
            {
                output.insert(schema.clone());
            }
        }
        PgType::Domain { schema, name, base } => {
            if !config
                .type_overrides
                .contains_key(&format!("{schema}.{name}"))
            {
                references(base, config, output);
            }
        }
        PgType::Array(base) => references(base, config, output),
        PgType::Builtin(_) => {}
    }
}
fn column_target(
    snapshot: &Snapshot,
    config: &Config,
    target: &str,
) -> Result<Option<String>, Error> {
    for schema in snapshot
        .schemas
        .iter()
        .filter(|schema| config.schemas.contains(&schema.name))
    {
        let schema_name = ident(&schema.name, false)?;
        for table in &schema.tables {
            let table_name = ident(&table.name, false)?;
            let computed = table.computed_fields.iter().map(|field| &field.name);
            for field in table
                .columns
                .iter()
                .map(|column| &column.name)
                .chain(computed)
            {
                let canonical = format!("{schema_name}.tables.{table_name}.{field}");
                if target == canonical
                    || target == format!("{schema_name}.{table_name}.{field}")
                    || target == format!("{}.tables.{}.{field}", schema.name, table.name)
                    || target == format!("{}.{}.{field}", schema.name, table.name)
                {
                    return Ok(Some(canonical));
                }
            }
        }
    }
    Ok(None)
}

fn included_schemas(
    snapshot: &Snapshot,
    config: &Config,
    field_overrides: &BTreeMap<String, Ident>,
) -> Result<BTreeSet<String>, Error> {
    let fields = json_field_types(snapshot, config)?;
    let mut included: BTreeSet<_> = config.schemas.iter().cloned().collect();
    if included.is_empty() {
        return Err(invalid("at least one schema must be selected"));
    }
    loop {
        let before = included.len();
        for name in included.clone() {
            if !snapshot.schemas.iter().any(|schema| schema.name == name) {
                return Err(invalid(format!("schema {name:?} is absent from snapshot")));
            }
            let prefix = format!("{}.", ident(&name, false)?);
            for (target, ty) in &fields {
                if target.starts_with(&prefix) && !field_overrides.contains_key(target) {
                    references(ty, config, &mut included);
                }
            }
        }
        if included.len() == before {
            return Ok(included);
        }
    }
}

fn validate_emitted_targets(
    snapshot: &Snapshot,
    config: &Config,
    included: &BTreeSet<String>,
) -> Result<(), Error> {
    let mut composite_prefixes = Vec::new();
    let mut selected_prefixes = Vec::new();
    for schema in &snapshot.schemas {
        let name = ident(&schema.name, false)?;
        if included.contains(&schema.name) {
            composite_prefixes.push(format!("{name}.composites."));
        }
        if config.schemas.contains(&schema.name) {
            selected_prefixes.push(format!("{name}.tables."));
            selected_prefixes.push(format!("{name}.functions."));
        }
    }
    for target in config.not_null.keys().chain(config.json_types.keys()) {
        if target.contains(".composites.")
            && !composite_prefixes
                .iter()
                .any(|prefix| target.starts_with(prefix))
            || !target.contains(".composites.")
                && config.not_null.contains_key(target)
                && !selected_prefixes
                    .iter()
                    .any(|prefix| target.starts_with(prefix))
        {
            return Err(invalid(format!(
                "customization target {target:?} is not emitted"
            )));
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "One schema traversal keeps emitted nesting and relative type paths together."
)]
pub fn generate(snapshot: &Snapshot, config: &Config) -> Result<String, Error> {
    if snapshot.version != SNAPSHOT_VERSION {
        return Err(invalid(format!(
            "unsupported snapshot version {}",
            snapshot.version
        )));
    }
    validate_relationships(snapshot)?;
    let runtime = syn::parse_str::<syn::Path>(&config.runtime_path)
        .map_err(|e| invalid(format!("invalid runtime path: {e}")))?;
    let prelude =
        syn::parse_file(&config.prelude).map_err(|e| invalid(format!("invalid prelude: {e}")))?;
    let derives = config
        .derives
        .iter()
        .map(|d| {
            syn::parse_str::<syn::Path>(d)
                .map_err(|e| invalid(format!("invalid derive {d:?}: {e}")))
        })
        .collect::<Result<_, _>>()?;
    let mut override_types: BTreeMap<String, syn::Type> = config
        .type_overrides
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                syn::parse_str::<syn::Type>(value)
                    .map_err(|e| invalid(format!("invalid override for {key}: {e}")))?,
            ))
        })
        .collect::<Result<_, Error>>()?;
    let mut canonical_fields = BTreeMap::new();
    for (target, value) in &config.column_types {
        let canonical = column_target(snapshot, config, target)?
            .ok_or_else(|| invalid(format!("unknown column_type target {target:?}")))?;
        if canonical_fields.contains_key(&canonical) {
            return Err(invalid(format!(
                "duplicate column_type target {canonical:?}"
            )));
        }
        let key = format!("field:{canonical}");
        override_types.insert(
            key.clone(),
            syn::parse_str::<syn::Type>(value)
                .map_err(|e| invalid(format!("invalid override for {target}: {e}")))?,
        );
        canonical_fields.insert(canonical, key);
    }
    let known_fields = json_field_types(snapshot, config)?;
    let mut json_targets = BTreeSet::new();
    for (target, value) in &config.json_types {
        let canonical = if known_fields.contains_key(target) {
            target.clone()
        } else {
            column_target(snapshot, config, target)?
                .or_else(|| {
                    known_fields
                        .keys()
                        .find(|canonical| canonical.replace(".tables.", ".") == *target)
                        .cloned()
                })
                .ok_or_else(|| invalid(format!("unknown json_type target {target:?}")))?
        };
        let ty = known_fields
            .get(&canonical)
            .ok_or_else(|| invalid(format!("unknown json_type target {target:?}")))?;
        if !is_json_type(ty) {
            return Err(invalid(format!(
                "json_type target {target:?} is not JSON-compatible"
            )));
        }
        if canonical_fields.contains_key(&canonical) {
            return Err(invalid(format!(
                "conflicting field overrides for {canonical:?}"
            )));
        }
        let key = format!("field:{canonical}");
        override_types.insert(
            key.clone(),
            syn::parse_str::<syn::Type>(value)
                .map_err(|error| invalid(format!("invalid json_type for {target}: {error}")))?,
        );
        json_targets.insert(canonical.clone());
        canonical_fields.insert(canonical, key);
    }
    let (aliases, overrides) = override_aliases(&prelude, override_types);
    let field_overrides = canonical_fields
        .into_iter()
        .filter_map(|(target, key)| overrides.get(&key).cloned().map(|alias| (target, alias)))
        .collect();
    let targeted = config
        .type_attributes
        .iter()
        .map(|(key, value)| Ok((key.clone(), attributes(value)?)))
        .collect::<Result<_, Error>>()?;
    unique(
        snapshot.schemas.iter().map(|s| s.name.as_str()),
        false,
        "schemas",
    )?;
    let schemas: BTreeMap<_, _> = snapshot
        .schemas
        .iter()
        .map(|s| (s.name.clone(), s))
        .collect();
    let selected: BTreeSet<_> = config.schemas.iter().cloned().collect();
    let included = included_schemas(snapshot, config, &field_overrides)?;
    validate_emitted_targets(snapshot, config, &included)?;
    let mut emitter = Emitter {
        config,
        runtime,
        derives,
        attributes: attributes(&config.attributes)?,
        targeted,
        used_targets: BTreeSet::new(),
        overrides,
        named: BTreeMap::new(),
        field_overrides,
        json_targets,
    };
    for name in &included {
        let schema = schemas
            .get(name)
            .ok_or_else(|| invalid(format!("schema {name:?} is absent from snapshot")))?;
        unique(
            schema.enums.iter().map(|e| e.name.as_str()),
            true,
            &format!("{name}.enums"),
        )?;
        unique(
            schema.composites.iter().map(|e| e.name.as_str()),
            true,
            &format!("{name}.composites"),
        )?;
        for (type_name, composite) in schema
            .enums
            .iter()
            .map(|e| (&e.name, false))
            .chain(schema.composites.iter().map(|c| (&c.name, true)))
        {
            if emitter
                .named
                .insert((name.clone(), type_name.clone()), composite)
                .is_some()
            {
                return Err(invalid(format!("duplicate named type {name}.{type_name}")));
            }
        }
    }
    let mut modules = TokenStream::new();
    let mut used_strict_functions = BTreeSet::new();
    let mut used_relationship_aliases = BTreeSet::new();
    for schema_name in included {
        let schema = schemas
            .get(&schema_name)
            .ok_or_else(|| invalid(format!("schema {schema_name:?} is absent from snapshot")))?;
        let schema_ident = ident(&schema_name, false)?;
        let mut enums = TokenStream::new();
        let mut sorted: Vec<_> = schema.enums.iter().collect();
        sorted.sort_by_key(|e| &e.name);
        for enumeration in sorted {
            unique(
                enumeration.variants.iter().map(String::as_str),
                true,
                &format!("enum {schema_name}.{}", enumeration.name),
            )?;
            let name = ident(&enumeration.name, true)?;
            let target = format!("{schema_ident}.enums.{name}");
            let decoration = emitter.decoration(&target, true, false);
            let mut variants = TokenStream::new();
            let mut display = TokenStream::new();
            for label in &enumeration.variants {
                let variant = ident(label, true)?;
                variants.extend(quote!(#[serde(rename = #label)] #variant,));
                display.extend(quote!(Self::#variant => formatter.write_str(#label),));
            }
            enums.extend(quote!(
                #decoration pub enum #name { #variants }
                impl ::core::fmt::Display for #name {
                    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        match self { #display }
                    }
                }
            ));
        }
        let mut composites = TokenStream::new();
        let mut sorted: Vec<_> = schema.composites.iter().collect();
        sorted.sort_by_key(|c| &c.name);
        for composite in sorted {
            let name = ident(&composite.name, true)?;
            let target = format!("{schema_ident}.composites.{name}");
            let decoration = emitter.decoration(&target, true, false);
            let columns = ColumnContract::resolve(&composite.fields, None, &target)?;
            let fields = emitter.fields(columns, 2, FieldMode::Row, &target)?;
            composites.extend(quote!(#decoration pub struct #name { #fields }));
        }
        unique(
            schema.tables.iter().map(|t| t.name.as_str()),
            false,
            &format!("{schema_name}.tables"),
        )?;
        let mut tables = TokenStream::new();
        let mut sorted: Vec<_> = schema
            .tables
            .iter()
            .filter(|_| selected.contains(&schema_name))
            .collect();
        sorted.sort_by_key(|t| &t.name);
        for table in sorted {
            let name = ident(&table.name, false)?;
            let target = format!("{schema_ident}.tables.{name}");
            let decoration = emitter.decoration(&format!("{target}.Row"), true, false);
            let columns: Vec<_> =
                ColumnContract::resolve(&table.columns, Some(table.kind), &target)?.collect();
            let fields = emitter.fields(columns.iter().copied(), 3, FieldMode::Row, &target)?;
            let runtime = emitter.runtime.clone();
            let relationship_markers = relationship_markers(
                schema,
                table,
                &runtime,
                config,
                &mut used_relationship_aliases,
            )?;
            let column_lookup = column_lookup(&schema_name, table, &runtime)?;
            let wire_name = &table.name;
            unique(
                table
                    .columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .chain(
                        table
                            .computed_fields
                            .iter()
                            .map(|field| field.name.as_str()),
                    ),
                false,
                &format!("{target} columns and computed fields"),
            )?;
            let mut writes = TokenStream::new();
            let mut column_markers = TokenStream::new();
            // Computed fields share the column contract for selection, filtering, and ordering,
            // but stay out of `Row` (`*` omits them) and the write payloads.
            let markers = columns
                .iter()
                .map(|contract| {
                    (
                        &contract.column.name,
                        &contract.column.ty,
                        contract.nullable,
                    )
                })
                .chain(
                    table
                        .computed_fields
                        .iter()
                        .map(|field| (&field.name, &field.ty, true)),
                );
            for (wire, ty, nullable) in markers {
                let column_name = ident(wire, false)?;
                let key = key_name(&column_name, &runtime);
                let selection = selection_identifier(wire);
                let base = emitter.field_ty(ty, &format!("{target}.{wire}"), 4)?;
                let (value, nullable_impl) = if nullable {
                    (
                        quote!(::std::option::Option<#base>),
                        quote!(impl #runtime::NullableColumn for #column_name {}),
                    )
                } else {
                    (base.clone(), TokenStream::new())
                };
                let json_impl = if is_json_column_type(ty) {
                    quote!(impl #runtime::JsonColumn for #column_name {})
                } else {
                    TokenStream::new()
                };
                column_markers.extend(quote!(
                    #[allow(non_camel_case_types)]
                    #[derive(::core::marker::Copy, ::core::clone::Clone)]
                    pub struct #column_name;
                    impl #runtime::Column for #column_name {
                        type Relation = super::Row;
                        type Value = #value;
                        type Filter = #base;
                        const NAME: &'static ::core::primitive::str = #wire;
                        const SELECT: &'static ::core::primitive::str = #selection;
                    }
                    impl #runtime::ColumnByKey<#key> for super::Row {
                        type Column = #column_name;
                        const COLUMN: Self::Column = #column_name;
                    }
                    #nullable_impl
                    #json_impl
                ));
            }
            if table.kind == TableKind::Table {
                for (rust_name, mode) in
                    [("Insert", FieldMode::Insert), ("Update", FieldMode::Update)]
                {
                    let default = columns
                        .iter()
                        .all(|column| mode.presence(column) != FieldPresence::Required);
                    let decoration =
                        emitter.decoration(&format!("{target}.{rust_name}"), false, default);
                    let fields = emitter.fields(columns.iter().copied(), 3, mode, &target)?;
                    let rust_name = Ident::new(rust_name, Span::call_site());
                    writes.extend(quote!(#decoration pub struct #rust_name { #fields }));
                }
                writes.extend(quote!(
                    impl #runtime::WritableRelation for Row {
                        type Insert = Insert;
                        type Update = Update;
                    }
                ));
            }
            tables.extend(quote!(
                pub mod #name {
                    #[allow(unused_imports)] use super::*;
                    #decoration pub struct Row { #fields }
                    impl #runtime::Relation for Row {
                        type ColumnLookup = ColumnLookup;
                        const SCHEMA: &'static ::core::primitive::str = #schema_name;
                        const NAME: &'static ::core::primitive::str = #wire_name;
                    }
                    impl #runtime::Projection<Self> for Row {
                        const SELECT_LEN: ::core::primitive::usize = 1;
                        fn write_selection(output: &mut ::std::string::String) {
                            output.push('*');
                        }
                        fn selection() -> ::std::borrow::Cow<'static, ::core::primitive::str> {
                            ::std::borrow::Cow::Borrowed("*")
                        }
                    }
                    pub fn query(client: #runtime::Postgrest) -> #runtime::Query<Row, Row> {
                        #runtime::query::<Row>(client)
                    }
                    #column_lookup
                    pub mod columns { #column_markers }
                    pub mod relationships { #relationship_markers }
                    #writes
                }
            ));
        }
        let mut functions = TokenStream::new();
        let mut groups: BTreeMap<&str, Vec<&Function>> = BTreeMap::new();
        for function in schema
            .functions
            .iter()
            .filter(|_| selected.contains(&schema_name))
        {
            groups.entry(&function.name).or_default().push(function);
        }
        let mut function_names = BTreeSet::new();
        for (wire_name, mut overloads) in groups {
            overloads.sort_by_key(|f| {
                f.arguments
                    .iter()
                    .map(|a| format!("{}:{:?}", a.name, a.ty))
                    .collect::<Vec<_>>()
            });
            let count = overloads.len();
            let mut signatures = BTreeSet::new();
            for (index, function) in overloads.into_iter().enumerate() {
                let signature = function
                    .arguments
                    .iter()
                    .map(|a| format!("{}:{:?}", a.name, a.ty))
                    .collect::<Vec<_>>();
                if !signatures.insert(signature) {
                    return Err(invalid(format!(
                        "duplicate RPC signature {schema_name}.{wire_name}"
                    )));
                }
                let base = ident(wire_name, false)?.to_string();
                let generated = if count == 1 {
                    base
                } else {
                    format!("{}_{index}", base.strip_prefix("r#").unwrap_or(&base))
                };
                if !function_names.insert(generated.clone()) {
                    return Err(invalid(format!(
                        "RPC module name collision {schema_name}.{generated}"
                    )));
                }
                let name =
                    syn::parse_str::<Ident>(&generated).map_err(|e| invalid(e.to_string()))?;
                let target = format!("{schema_ident}.functions.{name}");
                let strict = config.strict_args || config.strict_functions.contains(&target);
                used_strict_functions.insert(target.clone());
                unique(
                    function.arguments.iter().map(|a| a.name.as_str()),
                    false,
                    &target,
                )?;
                let decoration = emitter.decoration(
                    &format!("{target}.Args"),
                    false,
                    function.arguments.iter().all(|a| a.has_default),
                );
                let mut args = TokenStream::new();
                for argument in &function.arguments {
                    let field = ident(&argument.name, false)?;
                    let wire = &argument.name;
                    let field_target = format!("{target}.Args.{}", argument.name);
                    if argument.has_default {
                        let base = emitter.field_ty(&argument.ty, &field_target, 3)?;
                        let runtime = &emitter.runtime;
                        let skip = format!(
                            "{}::Field::is_omit",
                            quote!(#runtime).to_string().replace(' ', "")
                        );
                        args.extend(quote!(#[serde(rename = #wire, skip_serializing_if = #skip)] pub #field: #runtime::Field<::std::option::Option<#base>>,));
                    } else if strict && !argument.nullable {
                        let base = emitter.write_field_ty(&argument.ty, &field_target, 3)?;
                        args.extend(quote!(#[serde(rename = #wire)] pub #field: #base,));
                    } else {
                        let base = emitter.field_ty(&argument.ty, &field_target, 3)?;
                        args.extend(quote!(#[serde(rename = #wire)] pub #field: ::std::option::Option<#base>,));
                    }
                }
                let (record, returns) = emitter.function_returns(function, &target)?;
                let runtime = &emitter.runtime;
                functions.extend(quote!(pub mod #name { #[allow(unused_imports)] use super::*; #decoration pub struct Args { #args } #record pub type Returns = #returns; pub struct Function; impl #runtime::Function for Function { type Args = Args; type Returns = Returns; const SCHEMA: &'static str = #schema_name; const NAME: &'static str = #wire_name; } }));
            }
        }
        modules.extend(quote!(pub mod #schema_ident { #[allow(unused_imports)] use super::*; pub mod enums { #[allow(unused_imports)] use super::*; #enums } pub mod composites { #[allow(unused_imports)] use super::*; #composites } pub mod tables { #[allow(unused_imports)] use super::*; #tables } pub mod functions { #[allow(unused_imports)] use super::*; #functions } }));
    }
    for target in &config.strict_functions {
        if !used_strict_functions.contains(target) {
            return Err(invalid(format!(
                "unknown strict_args_for target {target:?}"
            )));
        }
    }
    for target in emitter.config.type_attributes.keys() {
        if !emitter.used_targets.contains(target) {
            return Err(invalid(format!(
                "unknown generated type attribute target {target:?}"
            )));
        }
    }
    for target in config.relationship_aliases.keys() {
        if !used_relationship_aliases.contains(target) {
            return Err(invalid(format!(
                "unknown relationship_alias target {target:?}"
            )));
        }
    }
    let macros = if config.reexport_macros {
        let runtime = &emitter.runtime;
        quote!(
            #[doc(hidden)]
            pub mod __supabase_codegen_runtime {
                pub use #runtime as schema;
                pub use #runtime::prelude::{select, key};
            }
            #[macro_export]
            macro_rules! select {
                ($($tokens:tt)*) => {
                    $crate::__supabase_codegen_runtime::select!(
                        runtime = $crate::__supabase_codegen_runtime; $($tokens)*
                    )
                };
            }
            #[macro_export]
            macro_rules! key {
                ($($tokens:tt)*) => {
                    $crate::__supabase_codegen_runtime::key!(
                        runtime = $crate::__supabase_codegen_runtime; $($tokens)*
                    )
                };
            }
        )
    } else {
        TokenStream::new()
    };
    let file = syn::parse2::<syn::File>(quote!(#prelude #aliases #modules #macros))
        .map_err(|e| invalid(format!("generated Rust is invalid: {e}")))?;
    Ok(prettyplease::unparse(&file))
}

#[cfg(test)]
#[path = "emitter_tests.rs"]
mod tests;
