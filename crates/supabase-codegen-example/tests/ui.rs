//! Pin the compiler output for common typed-query mistakes. Regenerate with `TRYBUILD=overwrite`.
#[test]
fn ui() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
