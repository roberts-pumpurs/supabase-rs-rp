#![expect(
    clippy::indexing_slicing,
    reason = "These fixtures construct exactly one schema and the indexed table before access."
)]
#![expect(
    clippy::unwrap_used,
    reason = "Unexpected generation failures must print their diagnostic in tests."
)]

use super::*;

fn snapshot() -> Snapshot {
    Snapshot {
        version: SNAPSHOT_VERSION,
        schemas: vec![Schema {
            name: "public".into(),
            enums: vec![],
            composites: vec![],
            tables: vec![],
            functions: vec![],
        }],
    }
}

fn column(name: &str) -> Column {
    Column {
        name: name.into(),
        ty: PgType::Builtin("text".into()),
        nullable: false,
        has_default: false,
        generated: false,
        identity: Identity::None,
    }
}

#[test]
fn normalized_name_collisions_and_unknown_attribute_targets_fail() {
    let mut metadata = snapshot();
    metadata.schemas[0].tables.push(Table {
        name: "messages".into(),
        kind: TableKind::Table,
        columns: vec![column("self"), column("Self")],
        primary_key: None,
        unique_keys: vec![],
        foreign_keys: vec![],
        is_partition: false,
        computed_fields: vec![],
    });
    let generator = crate::Generator::new();
    assert!(matches!(
        generator.clone().from_metadata(metadata.clone()),
        Err(Error::Invalid(_))
    ));
    metadata.schemas[0].tables[0].columns.pop();
    let generator = generator.type_attribute("public.tables.missing.Insert", "#[allow(dead_code)]");
    assert!(matches!(
        generator.from_metadata(metadata),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn computed_fields_cannot_collide_with_columns() {
    let mut metadata = snapshot();
    let mut messages = table("messages");
    messages.computed_fields.push(ComputedField {
        name: "id".into(),
        ty: PgType::Builtin("text".into()),
    });
    metadata.schemas[0].tables.push(messages);
    assert!(matches!(
        crate::Generator::new().from_metadata(metadata),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn output_is_stable_across_metadata_object_order() {
    let mut metadata = snapshot();
    for name in ["zebra", "alpha"] {
        metadata.schemas[0].tables.push(Table {
            name: name.into(),
            kind: TableKind::Table,
            columns: vec![column("value")],
            primary_key: None,
            unique_keys: vec![],
            foreign_keys: vec![],
            is_partition: false,
            computed_fields: vec![],
        });
    }
    let generator = crate::Generator::new();
    let first = generator.clone().from_metadata(metadata.clone()).unwrap();
    metadata.schemas[0].tables.reverse();
    assert_eq!(
        first.source(),
        generator.from_metadata(metadata).unwrap().source()
    );
}

#[test]
fn unsupported_types_fail_without_a_mapping() {
    let mut metadata = snapshot();
    let mut value = column("value");
    value.ty = PgType::Named {
        schema: "extensions".into(),
        name: "opaque".into(),
    };
    metadata.schemas[0].tables.push(Table {
        name: "messages".into(),
        kind: TableKind::Table,
        columns: vec![value],
        primary_key: None,
        unique_keys: vec![],
        foreign_keys: vec![],
        is_partition: false,
        computed_fields: vec![],
    });
    assert!(matches!(
        crate::Generator::new().from_metadata(metadata),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn invalid_custom_rust_is_rejected_before_emission() {
    let generators = [
        crate::Generator::new().prelude("use broken::;"),
        crate::Generator::new().derive("not a path"),
        crate::Generator::new().attribute("#[broken("),
        crate::Generator::new().type_override("pg_catalog.text", "Vec<"),
        crate::Generator::new().runtime_path("not a path"),
    ];
    for generator in generators {
        assert!(matches!(
            generator.from_metadata(snapshot()),
            Err(Error::Invalid(_))
        ));
    }
}

fn table(name: &str) -> Table {
    Table {
        name: name.into(),
        kind: TableKind::Table,
        columns: vec![column("id"), column("parent_id")],
        primary_key: Some(vec!["id".into()]),
        unique_keys: vec![],
        foreign_keys: vec![],
        is_partition: false,
        computed_fields: vec![],
    }
}

fn foreign_key(name: &str, target: &str) -> ForeignKey {
    ForeignKey {
        name: name.into(),
        columns: vec!["parent_id".into()],
        referenced_relation: RelationRef {
            schema: "public".into(),
            name: target.into(),
        },
        referenced_columns: vec!["id".into()],
    }
}

#[test]
fn direct_relationships_have_stable_identity_and_conservative_cardinality() {
    let mut metadata = snapshot();
    let mut orders = table("orders");
    orders
        .foreign_keys
        .push(foreign_key("orders_customer", "customers"));
    metadata.schemas[0].tables = vec![orders, table("customers")];
    let config = crate::Generator::new().config;
    let generated = generate(&metadata, &config).unwrap();
    assert!(generated.contains("pub struct orders_customer;"));
    assert!(generated.contains("pub struct orders_orders_customer;"));
    assert!(generated.contains("type Cardinality = ::rp_supabase_client::schema::ToMany;"));
    assert!(generated.contains("const RESOURCE: &'static ::core::primitive::str = \"customers\";"));
    assert!(
        generated.contains("const HINT: &'static ::core::primitive::str = \"orders_customer\";")
    );
    metadata.schemas[0].tables.reverse();
    assert_eq!(generated, generate(&metadata, &config).unwrap());
    metadata.schemas[0].tables[1]
        .unique_keys
        .push(vec!["parent_id".into()]);
    assert!(!generate(&metadata, &config).unwrap().contains("::ToMany"));
}

#[test]
fn composite_reverse_uniqueness_compares_sets_not_order() {
    let mut metadata = snapshot();
    let mut source = table("source");
    source
        .unique_keys
        .push(vec!["id".into(), "parent_id".into()]);
    let mut target = table("target");
    target.primary_key = Some(vec!["id".into(), "parent_id".into()]);
    let mut edge = foreign_key("paired", "target");
    edge.columns = vec!["parent_id".into(), "id".into()];
    edge.referenced_columns = vec!["id".into(), "parent_id".into()];
    source.foreign_keys.push(edge);
    metadata.schemas[0].tables = vec![source, target];
    assert!(
        !generate(&metadata, &crate::Generator::new().config)
            .unwrap()
            .contains("::ToMany")
    );
}

#[test]
fn partition_self_and_external_edges_are_not_emitted() {
    let mut metadata = snapshot();
    let mut source = table("source");
    source.foreign_keys.push(foreign_key("self_edge", "source"));
    source
        .foreign_keys
        .push(foreign_key("partition_edge", "partition"));
    source
        .foreign_keys
        .push(foreign_key("missing_edge", "absent"));
    let mut external = foreign_key("external_edge", "elsewhere");
    external.referenced_relation.schema = "external".into();
    source.foreign_keys.push(external);
    let mut partition = table("partition");
    partition.is_partition = true;
    metadata.schemas[0].tables = vec![source, partition];
    let generated = generate(&metadata, &crate::Generator::new().config).unwrap();
    for name in [
        "self_edge",
        "partition_edge",
        "missing_edge",
        "external_edge",
    ] {
        let marker = format!("pub struct {name};");
        assert!(!generated.contains(&marker));
    }
}

#[test]
fn malformed_keys_and_foreign_keys_fail_generation() {
    let mut metadata = snapshot();
    let mut source = table("source");
    source.foreign_keys.push(foreign_key("edge", "target"));
    metadata.schemas[0].tables = vec![source, table("target")];
    let config = crate::Generator::new().config;
    let mut invalid_cases = Vec::new();
    let mut case = metadata.clone();
    case.schemas[0].tables[0].primary_key = Some(vec![]);
    invalid_cases.push(case);
    let mut case = metadata.clone();
    case.schemas[0].tables[0].unique_keys = vec![vec!["absent".into()]];
    invalid_cases.push(case);
    let mut case = metadata.clone();
    case.schemas[0].tables[0].foreign_keys[0]
        .referenced_columns
        .clear();
    invalid_cases.push(case);
    let mut case = metadata.clone();
    case.schemas[0].tables[0].foreign_keys[0].columns = vec!["absent".into()];
    invalid_cases.push(case);
    let mut case = metadata.clone();
    case.schemas[0].tables[0].foreign_keys[0].referenced_columns = vec!["absent".into()];
    invalid_cases.push(case);
    let mut case = metadata.clone();
    let duplicate = case.schemas[0].tables[0].foreign_keys[0].clone();
    case.schemas[0].tables[0].foreign_keys.push(duplicate);
    invalid_cases.push(case);
    for case in invalid_cases {
        assert!(matches!(generate(&case, &config), Err(Error::Invalid(_))));
    }
}

#[test]
fn relationship_marker_normalization_collisions_fail() {
    let mut metadata = snapshot();
    let mut source = table("source");
    source.foreign_keys = vec![
        foreign_key("same-name", "target"),
        foreign_key("same_name", "target"),
    ];
    metadata.schemas[0].tables = vec![source, table("target")];
    assert!(matches!(
        generate(&metadata, &crate::Generator::new().config),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn identifiers_quote_grammar_and_control_names() {
    for name in [
        "*",
        "order",
        "select",
        "columns",
        "on_conflict",
        "limit",
        "offset",
        "and",
        "or",
        "a:b",
        "a!b",
        "a,b",
        "\u{65e5}\u{672c}\u{8a9e}",
    ] {
        let expected = format!("\"{name}\"");
        assert_eq!(selection_identifier(name), expected);
    }
    assert_eq!(selection_identifier("a\"b\\c"), "\"a\\\"b\\\\c\"");
    assert_eq!(selection_identifier("simple_name"), "simple_name");
}

#[test]
fn dependency_schemas_only_emit_types_not_endpoints_or_relationships() {
    let mut metadata = snapshot();
    let mut source = table("source");
    source.columns[0].ty = PgType::Named {
        schema: "dependency".into(),
        name: "status".into(),
    };
    let mut edge = foreign_key("external", "hidden_endpoint");
    edge.referenced_relation.schema = "dependency".into();
    source.foreign_keys.push(edge);
    metadata.schemas[0].tables.push(source);
    metadata.schemas.push(Schema {
        name: "dependency".into(),
        enums: vec![Enum {
            name: "status".into(),
            variants: vec!["ready".into()],
        }],
        composites: vec![],
        tables: vec![table("hidden_endpoint")],
        functions: vec![Function {
            name: "hidden_rpc".into(),
            arguments: vec![],
            returns: ReturnType::Type(PgType::Builtin("text".into())),
            returns_set: false,
        }],
    });
    let config = crate::Generator::new().config;
    let generated = generate(&metadata, &config).unwrap();
    assert!(generated.contains("pub enum Status"));
    assert!(!generated.contains("pub mod hidden_endpoint"));
    assert!(!generated.contains("pub struct external;"));
    assert!(!generated.contains("pub mod hidden_rpc"));
    let mut selected = config;
    selected.schemas.push("dependency".into());
    let generated = generate(&metadata, &selected).unwrap();
    assert!(generated.contains("pub mod hidden_endpoint"));
    assert!(generated.contains("pub mod hidden_rpc"));
    assert!(!generated.contains("pub struct external;"));
}

#[test]
fn not_null_customization_persists_and_rejects_unknown_fields() {
    let mut metadata = snapshot();
    metadata.schemas[0].functions.push(Function {
        name: "finalize".into(),
        arguments: vec![],
        returns: ReturnType::Record(vec![Column {
            nullable: true,
            ..column("id")
        }]),
        returns_set: false,
    });
    let bindings = crate::Generator::new()
        .not_null("public.functions.finalize.Record", ["id"])
        .from_metadata(metadata.clone())
        .unwrap();
    assert!(matches!(
        &bindings.snapshot().schemas[0].functions[0].returns,
        ReturnType::Record(fields) if !fields[0].nullable
    ));
    crate::Generator::new()
        .not_null("public.functions.finalize.Record", ["missing"])
        .from_metadata(metadata.clone())
        .unwrap_err();
    crate::Generator::new()
        .not_null("public.functions.missing.Record", ["id"])
        .from_metadata(metadata)
        .unwrap_err();
}

#[test]
fn column_overrides_validate_sql_targets_and_rust_types() {
    let mut metadata = snapshot();
    metadata.schemas[0].tables.push(table("videos"));
    for (target, ty) in [
        ("public.videos.missing", "String"),
        ("public.missing.id", "String"),
        ("public.videos.id", "Vec<"),
    ] {
        crate::Generator::new()
            .column_type(target, ty)
            .from_metadata(metadata.clone())
            .unwrap_err();
    }
}

#[test]
fn json_customization_rejects_non_json_and_unknown_targets() {
    let mut metadata = snapshot();
    metadata.schemas[0].tables.push(table("items"));
    for target in ["public.tables.items.id", "public.functions.missing.Returns"] {
        crate::Generator::new()
            .json_type(target, "crate::Outcome")
            .from_metadata(metadata.clone())
            .unwrap_err();
    }
}

#[test]
fn relationship_aliases_reject_unknown_edges_and_collisions() {
    let mut metadata = snapshot();
    let mut orders = table("orders");
    orders
        .foreign_keys
        .push(foreign_key("orders_customer", "customers"));
    metadata.schemas[0].tables = vec![orders, table("customers")];
    for (target, alias) in [
        ("public.tables.orders.relationships.missing", "customer"),
        (
            "public.tables.orders.relationships.orders_customer",
            "orders_customer",
        ),
        (
            "public.tables.orders.relationships.orders_customer",
            "parent",
        ),
        (
            "public.tables.orders.relationships.orders_customer",
            "not valid",
        ),
    ] {
        crate::Generator::new()
            .relationship_alias(target, alias)
            .from_metadata(metadata.clone())
            .unwrap_err();
    }
}

#[test]
fn strict_function_targets_are_checked() {
    crate::Generator::new()
        .strict_args_for("public.functions.missing")
        .from_metadata(snapshot())
        .unwrap_err();
}

#[test]
fn customizations_require_emitted_targets_but_allow_dependency_composites() {
    let mut metadata = snapshot();
    metadata.schemas.push(Schema {
        name: "dependency".into(),
        enums: vec![],
        composites: vec![Composite {
            name: "detail".into(),
            fields: vec![Column {
                ty: PgType::Builtin("jsonb".into()),
                nullable: true,
                ..column("data")
            }],
        }],
        tables: vec![Table {
            kind: TableKind::View,
            ..table("hidden")
        }],
        functions: vec![Function {
            name: "hidden".into(),
            arguments: vec![],
            returns: ReturnType::Record(vec![column("id")]),
            returns_set: false,
        }],
    });
    for target in [
        "dependency.tables.hidden.Row",
        "dependency.functions.hidden.Record",
        "dependency.composites.Detail",
    ] {
        let field = if target.ends_with("Detail") {
            "data"
        } else {
            "id"
        };
        crate::Generator::new()
            .not_null(target, [field])
            .from_metadata(metadata.clone())
            .unwrap_err();
    }
    crate::Generator::new()
        .json_type("dependency.composites.Detail.data", "String")
        .from_metadata(metadata.clone())
        .unwrap_err();
    let mut relation = table("items");
    relation.columns[0].ty = PgType::Named {
        schema: "dependency".into(),
        name: "detail".into(),
    };
    metadata.schemas[0].tables.push(relation);
    let bindings = crate::Generator::new()
        .not_null("dependency.composites.Detail", ["data"])
        .json_type("dependency.composites.Detail.data", "String")
        .from_metadata(metadata)
        .unwrap();
    assert!(!bindings.snapshot().schemas[1].composites[0].fields[0].nullable);
}
