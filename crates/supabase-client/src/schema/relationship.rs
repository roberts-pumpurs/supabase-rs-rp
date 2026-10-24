//! Typed relationship selections and scoped filter paths.
use super::{Column, Projection, Relation};
use core::marker::PhantomData;
use serde::de::DeserializeOwned;

/// Conservative response cardinality of a direct relationship.
pub trait Cardinality {
    /// Decoded child response.
    type Output<P: DeserializeOwned>: DeserializeOwned;
}
/// A nullable single related row, even for an inner embed.
#[derive(Clone, Copy)]
pub struct ToOne;
impl Cardinality for ToOne {
    type Output<P: DeserializeOwned> = Option<P>;
}
/// Zero or more related rows.
#[derive(Clone, Copy)]
pub struct ToMany;
impl Cardinality for ToMany {
    type Output<P: DeserializeOwned> = Vec<P>;
}
/// A generated direct foreign-key relationship.
pub trait Relationship: Copy {
    /// Source relation.
    type Source: Relation;
    /// Target relation.
    type Target: Relation + Projection<Self::Target>;
    /// Conservative response cardinality.
    type Cardinality: Cardinality;
    /// Escaped target resource.
    const RESOURCE: &'static str;
    /// Escaped foreign-key constraint name.
    const HINT: &'static str;
}
/// A projection-owned selected relationship path.
pub trait EmbedPath {
    /// Exact owning projection.
    type Owner;
    /// Source relation of the first selected relationship.
    type Source: Relation;
    /// Selected child projection (or a predicate-only marker).
    type Selected;
    /// Final target relation.
    type Target: Relation;
    /// Exact byte length of the escaped alias path.
    #[must_use]
    fn path_len(&self) -> usize;
    /// Append the escaped alias path.
    fn write_path(&self, output: &mut String);
}
/// Predicate-only selection identity; it has no selected child handles.
pub struct EmptySelection<T>(PhantomData<fn() -> T>);
/// A relationship handle owned by a named projection.
#[must_use]
pub struct Embed<O, P, E> {
    alias: &'static str,
    length: usize,
    #[expect(
        clippy::type_complexity,
        reason = "Function markers retain identity without imposing ownership or auto-trait bounds."
    )]
    marker: PhantomData<fn() -> (O, P, E)>,
}
impl<O, P, E> Copy for Embed<O, P, E> {}
impl<O, P, E> Clone for Embed<O, P, E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<O, P, E: Relationship> Embed<O, P, E> {
    /// Constructor used by the projection macro.
    #[doc(hidden)]
    pub const fn new(alias: &'static str) -> Self {
        Self {
            alias,
            length: identifier_len(alias),
            marker: PhantomData,
        }
    }
    /// Compose with a handle belonging to this exact selected child.
    pub const fn then<H: EmbedPath<Owner = P, Source = E::Target>>(self, next: H) -> Path<Self, H> {
        Path { first: self, next }
    }
}
impl<O, P, E: Relationship> EmbedPath for Embed<O, P, E> {
    type Owner = O;
    type Source = E::Source;
    type Selected = P;
    type Target = E::Target;
    fn path_len(&self) -> usize {
        self.length
    }
    fn write_path(&self, output: &mut String) {
        write_identifier(output, self.alias);
    }
}
/// A composed selected relationship path.
#[must_use]
#[derive(Clone, Copy)]
pub struct Path<A, B> {
    first: A,
    next: B,
}
impl<A: EmbedPath, B: EmbedPath<Owner = A::Selected, Source = A::Target>> Path<A, B> {
    /// Construct a composed selected path.
    #[doc(hidden)]
    pub const fn new(first: A, next: B) -> Self {
        Self { first, next }
    }
    /// Resolve a column on the final target relation.
    #[expect(
        clippy::unused_self,
        reason = "The fluent path supplies the final target type for column inference."
    )]
    pub fn column<K, N, C>(self, _: super::Key<K, N>) -> C
    where
        N: super::KeyName<Key = K>,
        B::Target: super::selection::SelectedColumn<N, Column = C>,
    {
        <B::Target as super::selection::SelectedColumn<N>>::COLUMN
    }
    /// Compose another handle from the final selected child.
    pub const fn then<H: EmbedPath<Owner = B::Selected, Source = B::Target>>(
        self,
        next: H,
    ) -> Path<Self, H> {
        Path { first: self, next }
    }
}
impl<A: EmbedPath, B: EmbedPath<Owner = A::Selected, Source = A::Target>> EmbedPath for Path<A, B> {
    type Owner = A::Owner;
    type Source = A::Source;
    type Selected = B::Selected;
    type Target = B::Target;
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Selected static alias paths fit their request buffer capacity."
    )]
    fn path_len(&self) -> usize {
        self.first.path_len() + 1 + self.next.path_len()
    }
    fn write_path(&self, output: &mut String) {
        self.first.write_path(output);
        output.push('.');
        self.next.write_path(output);
    }
}
/// Remove the Rust raw-identifier prefix without allocating.
#[doc(hidden)]
#[must_use]
#[expect(
    clippy::panic,
    reason = "Removing an ASCII prefix from a valid str preserves its UTF-8 boundary."
)]
pub const fn alias(name: &'static str) -> &'static str {
    match name.as_bytes() {
        [b'r', b'#', alias @ ..] => match core::str::from_utf8(alias) {
            Ok(alias) => alias,
            Err(_) => panic!("raw-identifier prefix must end at a UTF-8 boundary"),
        },
        _ => name,
    }
}
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !matches!(
            name,
            "select" | "columns" | "on_conflict" | "order" | "limit" | "offset" | "and" | "or"
        )
}
/// Append one PostgREST identifier, escaping grammar punctuation.
#[doc(hidden)]
pub fn write_identifier(output: &mut String, name: &str) {
    if plain(name) {
        output.push_str(name);
    } else {
        output.push('"');
        for character in name.chars() {
            if character == '"' || character == '\\' {
                output.push('\\');
            }
            output.push(character);
        }
        output.push('"');
    }
}
/// Capacity contribution of an escaped alias.
#[doc(hidden)]
#[must_use]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "Const loops stay within slice bounds; escaped static identifiers fit selection capacity."
)]
pub const fn identifier_len(name: &str) -> usize {
    let bytes = name.as_bytes();
    let mut index = 0;
    let mut simple = !bytes.is_empty();
    let mut escapes = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !((byte >= b'a' && byte <= b'z')
            || (byte >= b'A' && byte <= b'Z')
            || (byte >= b'0' && byte <= b'9')
            || byte == b'_')
        {
            simple = false;
        }
        if byte == b'"' || byte == b'\\' {
            escapes += 1;
        }
        index += 1;
    }
    let controls = [
        "select",
        "columns",
        "on_conflict",
        "order",
        "limit",
        "offset",
        "and",
        "or",
    ];
    let mut control = 0;
    while control < controls.len() {
        if equal(name, controls[control]) {
            simple = false;
        }
        control += 1;
    }
    if simple {
        bytes.len()
    } else {
        bytes.len() + escapes + 2
    }
}
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "Both byte slices have equal length and the const loop remains within it."
)]
const fn equal(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    if left_bytes.len() != right_bytes.len() {
        return false;
    }
    let mut index = 0;
    while index < left_bytes.len() {
        if left_bytes[index] != right_bytes[index] {
            return false;
        }
        index += 1;
    }
    true
}
/// Reject ambiguous response and selection aliases at compile time.
#[doc(hidden)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "Const loop indices remain within the static projection key slice."
)]
pub const fn assert_distinct(keys: &[&str]) {
    let mut left = 0;
    while left < keys.len() {
        let mut right = left + 1;
        while right < keys.len() {
            assert!(
                !equal(keys[left], keys[right]),
                "duplicate projection response alias"
            );
            right += 1;
        }
        left += 1;
    }
}
/// Validate source and selected target identities without runtime work.
#[doc(hidden)]
pub const fn check_embed<R: Relation, E: Relationship<Source = R>, P: Projection<E::Target>>() {}
/// Validate a predicate-only source identity.
#[doc(hidden)]
pub const fn check_empty<R: Relation, E: Relationship<Source = R>>() {}
/// Validate exact selected value types and SQL response keys without runtime work.
#[doc(hidden)]
pub const fn assert_same_column<A: Column, B: Column<Value = A::Value>>() {
    assert!(
        equal(A::NAME, B::NAME),
        "shared projection response keys differ"
    );
}
