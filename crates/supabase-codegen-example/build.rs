use rp_supabase_codegen::Generator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=schema.json");
    println!("cargo:rerun-if-changed=tests/schema.json");
    println!("cargo:rerun-if-env-changed=SUPABASE_CODEGEN_DATABASE_URL");
    let generator = Generator::new()
        .schema("public")
        .prelude("use ::std::string::String as DatabaseText;")
        .derive("PartialEq")
        .attribute("#[allow(dead_code)]")
        .type_override("pg_catalog.text", "DatabaseText")
        .type_attribute(
            "public.tables.messages.Insert",
            "#[derive(::typed_builder::TypedBuilder)]",
        )
        .type_attribute(
            "public.tables.messages.Insert",
            "#[builder(field_defaults(setter(into)))]",
        );
    // Live metadata is an explicit opt-in. Invalid credentials never fall back to the snapshot.
    let bindings = if std::env::var_os("SUPABASE_CODEGEN_DATABASE_URL").is_some() {
        generator.from_database_env("SUPABASE_CODEGEN_DATABASE_URL")?
    } else {
        generator.from_snapshot("schema.json")?
    };
    bindings.write_to_out_dir("database.rs")?;
    // Compile consumer regressions using an alias that deliberately shares Row's name.
    Generator::new()
        .schemas(["public", "shared"])
        .prelude("use ::std::string::String as Row; #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)] pub enum AuthorKind { #[serde(rename = \"user\")] User } #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)] pub struct Outcome { pub ok: bool }")
        .type_override("pg_catalog.text", "Row")
        .column_type("public.check_probe.external_author", "AuthorKind")
        .json_type("public.tables.json_probe.document", "Outcome")
        .json_type("public.tables.json_probe.documents", "Outcome")
        .json_type("public.composites.JsonInfo.data", "Outcome")
        .json_type("public.tables.json_probe.outcome", "Outcome")
        .json_type("public.functions.fetch_outcome.Args.audience", "Outcome")
        .json_type("public.functions.fetch_outcome.Returns", "Outcome")
        .json_type("public.functions.outcome_records.Record.data", "Outcome")
        .relationship_alias("public.tables.orders.relationships.orders_customer", "buyer")
        .strict_args_for("public.functions.strict_probe")
        .json_type("public.functions.strict_probe.Args.document", "Outcome")
        .json_type("public.functions.strict_probe.Args.payload", "Outcome")
        .from_snapshot("tests/schema.json")?
        .write_to_out_dir("contract_bindings.rs")?;
    Generator::new()
        .schemas(["public", "shared"])
        .reexport_macros()
        .from_snapshot("tests/schema.json")?
        .write_to_out_dir("macro_bindings.rs")?;
    for (runtime, output) in [
        ("Name", "runtime_name.rs"),
        ("Selected", "runtime_selected.rs"),
        ("__supabase_column_runtime", "runtime_prefixed.rs"),
    ] {
        Generator::new()
            .schema("public")
            .runtime_path(format!("{runtime}::schema"))
            .from_snapshot("schema.json")?
            .write_to_out_dir(output)?;
    }
    Ok(())
}
