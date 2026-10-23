//! Typed named and query-local selection descriptors and version-owned macro support.
use super::{
    Cardinality, Column, ColumnByKey, EmbedPath, Key, KeyName, Path, Projection, Relation,
    Relationship, RelationshipByKey,
};
use core::marker::PhantomData;
/// A copyable selected shape with an exact decoded record and relation.
pub trait Selection: Copy {
    /// Selected root relation.
    type Relation: Relation + Projection<Self::Relation>;
    /// Decoded selected shape.
    type Record: Projection<Self::Relation>;
    /// Start a query using this selected shape.
    fn query(self, client: super::Postgrest) -> super::Query<Self::Relation, Self::Record> {
        super::query::<Self::Relation>(client).select(self)
    }
    /// Strictly decode a response array.
    /// # Errors
    /// Returns JSON syntax, missing-field and duplicate-field errors.
    fn decode(self, body: &str) -> Result<Vec<Self::Record>, serde_json::Error> {
        serde_json::from_str(body)
    }
    /// Resolve a column on the selected root relation.
    fn column<K>(self, _: Key<K>) -> <Self::Relation as ColumnByKey<K>>::Column
    where
        Self::Relation: ColumnByKey<K>,
    {
        <Self::Relation as ColumnByKey<K>>::COLUMN
    }
}
/// Select an existing named projection without storing any decoded data.
#[must_use]
pub const fn named<R: Relation + Projection<R>, P: Projection<R>>() -> Named<R, P> {
    Named(PhantomData)
}
/// A static response alias implemented by generated selection descriptors.
pub trait Alias {
    /// Normalized response alias.
    const NAME: &'static str;
}
/// A selected relationship handle. Inline handles and children are zero-sized.
/// Named projection paths may additionally store static alias strings.
#[expect(
    clippy::partial_pub_fields,
    reason = "Children are public for typed composition; the private marker preserves handle ownership."
)]
pub struct Handle<O, P, E, A, D> {
    /// Typed child selection; named children compose using their DTO-owned handles.
    pub child: D,
    #[expect(
        clippy::type_complexity,
        reason = "Function marker preserves handle identities without decoded-data bounds."
    )]
    marker: PhantomData<fn() -> (O, P, E, A)>,
}
impl<O, P, E, A, D: Copy> Copy for Handle<O, P, E, A, D> {}
impl<O, P, E, A, D: Copy> Clone for Handle<O, P, E, A, D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<O, P, E: Relationship, A: Alias, D> Handle<O, P, E, A, D> {
    /// Construct a generated relationship handle.
    #[doc(hidden)]
    pub const fn new(child: D) -> Self {
        Self {
            child,
            marker: PhantomData,
        }
    }
    /// Compose a handle owned by this exact selected child.
    pub const fn then<H: EmbedPath<Owner = P, Source = E::Target>>(self, next: H) -> Path<Self, H> {
        Path::new(self, next)
    }
    /// Resolve a column on the final target relation.
    #[expect(
        clippy::unused_self,
        reason = "The fluent handle supplies the final target type for column inference."
    )]
    pub fn column<K>(self, _: Key<K>) -> <E::Target as ColumnByKey<K>>::Column
    where
        E::Target: ColumnByKey<K>,
    {
        <E::Target as ColumnByKey<K>>::COLUMN
    }
}
impl<O, P, E: Relationship, A: Alias, D> EmbedPath for Handle<O, P, E, A, D> {
    type Owner = O;
    type Source = E::Source;
    type Selected = P;
    type Target = E::Target;
    fn path_len(&self) -> usize {
        super::__private::identifier_len(A::NAME)
    }
    fn write_path(&self, output: &mut String) {
        super::__private::write_identifier(output, A::NAME);
    }
}
/// Resolved selection metadata used by generated constructors.
#[doc(hidden)]
pub struct RelationToken<R>(PhantomData<fn() -> R>);
impl<R> Copy for RelationToken<R> {}
impl<R> Clone for RelationToken<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> RelationToken<R> {
    /// Construct a zero-sized metadata token.
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}
impl<R> Default for RelationToken<R> {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved selection metadata used by generated constructors.
#[doc(hidden)]
pub struct EdgeToken<E>(PhantomData<fn() -> E>);
impl<E> Copy for EdgeToken<E> {}
impl<E> Clone for EdgeToken<E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<E: Relationship> EdgeToken<E> {
    /// Resolve or construct checked selection metadata.
    #[doc(hidden)]
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "The fluent edge token supplies its relationship target for descriptor inference."
    )]
    pub const fn target(self) -> RelationToken<E::Target> {
        RelationToken::new()
    }
}

// These are the only schema-key lookup sites. Generated records never contain ColumnOf/EdgeOf.
//
// `select!` passes a block-local unit struct named after the identifier the user wrote.
// rustc prints that type in the diagnostics below, so an unknown name appears as a word.
// `do_not_recommend` stops rustc from reporting the encoded `ColumnByKey` key instead.
/// Column lookup for `select!`. Implemented for every relation that resolves the key.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "no column `{Name}` on `{Self}`",
    label = "unknown column",
    note = "valid names are the items of the `columns` module next to `{Self}`"
)]
pub trait SelectedColumn<Name>: Relation {
    /// Generated column marker.
    type Column: Column<Relation = Self>;
}
#[diagnostic::do_not_recommend]
impl<R: ColumnByKey<N::Key>, N: KeyName> SelectedColumn<N> for R {
    type Column = R::Column;
}
/// Relationship lookup for `select!`. Implemented for every relation that resolves the key.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "no relationship `{Name}` on `{Self}`",
    label = "unknown relationship",
    note = "valid names are the items of the `relationships` module next to `{Self}`"
)]
pub trait SelectedRelationship<Name>: Relation {
    /// Generated relationship marker.
    type Edge: Relationship<Source = Self>;
}
#[diagnostic::do_not_recommend]
impl<R: RelationshipByKey<N::Key>, N: KeyName> SelectedRelationship<N> for R {
    type Edge = R::Edge;
}
/// Resolve or construct checked selection metadata.
// The output is a separate parameter: a `R::Column` projection in the return type
// would report a second, encoded copy of an unknown-column error.
#[doc(hidden)]
#[must_use]
pub fn column<R: SelectedColumn<N, Column = C>, N, C>(_: RelationToken<R>, _: N) -> Scalar<C> {
    Scalar(PhantomData)
}
/// Resolve or construct checked selection metadata.
#[doc(hidden)]
#[must_use]
pub fn edge<R: SelectedRelationship<N, Edge = E>, N, E>(_: RelationToken<R>, _: N) -> EdgeToken<E> {
    EdgeToken(PhantomData)
}
/// Resolved selection metadata used by generated constructors.
#[doc(hidden)]
pub struct Scalar<C>(PhantomData<fn() -> C>);
impl<C> Copy for Scalar<C> {}
impl<C> Clone for Scalar<C> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Resolved selection descriptor contract.
/// Named and query-local projection compilation both render through these descriptors.
#[doc(hidden)]
pub trait SelectionField<R: Relation>: Copy {
    /// Resolved descriptor component.
    type Value: serde::de::DeserializeOwned;
    /// Resolved descriptor component.
    const KEY: &'static str;
    /// Resolved descriptor component.
    const LEN: usize;
    /// Resolved descriptor component.
    fn write(output: &mut String);
}
impl<C: Column> SelectionField<C::Relation> for Scalar<C> {
    /// Resolved descriptor component.
    type Value = C::Value;
    /// Resolved descriptor component.
    const KEY: &'static str = C::NAME;
    /// Resolved descriptor component.
    const LEN: usize = C::SELECT.len();
    /// Resolved descriptor component.
    fn write(output: &mut String) {
        output.push_str(C::SELECT);
    }
}

/// Resolved selection metadata used by generated constructors.
#[doc(hidden)]
pub struct Named<R, P>(PhantomData<fn() -> (R, P)>);
impl<R, P> Copy for Named<R, P> {}
impl<R, P> Clone for Named<R, P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R: Relation + Projection<R>, P: Projection<R>> Selection for Named<R, P> {
    type Relation = R;
    type Record = P;
}
/// Resolve or construct checked selection metadata.
#[doc(hidden)]
#[must_use]
pub fn shared<E: Relationship, P: Projection<E::Target>>(_: EdgeToken<E>) -> Named<E::Target, P> {
    Named(PhantomData)
}
/// Resolved selection metadata used by generated constructors.
#[doc(hidden)]
pub struct Child<E, D, A, const INNER: bool> {
    child: D,
    marker: PhantomData<fn() -> (E, A)>,
}
impl<E, D: Copy, A, const INNER: bool> Copy for Child<E, D, A, INNER> {}
impl<E, D: Copy, A, const INNER: bool> Clone for Child<E, D, A, INNER> {
    fn clone(&self) -> Self {
        *self
    }
}
/// Resolve or construct checked selection metadata.
#[doc(hidden)]
pub fn embed<E: Relationship, D: Selection<Relation = E::Target>, A: Alias, const INNER: bool>(
    _: EdgeToken<E>,
    child: D,
    _: A,
) -> Child<E, D, A, INNER> {
    Child {
        child,
        marker: PhantomData,
    }
}
impl<E: Relationship, D: Selection<Relation = E::Target>, A: Alias, const INNER: bool>
    SelectionField<E::Source> for Child<E, D, A, INNER>
{
    /// Resolved descriptor component.
    type Value = <E::Cardinality as Cardinality>::Output<D::Record>;
    /// Resolved descriptor component.
    const KEY: &'static str = A::NAME;
    /// Resolved descriptor component.
    const LEN: usize = super::__private::identifier_len(A::NAME)
        + E::RESOURCE.len()
        + E::HINT.len()
        + <D::Record as Projection<E::Target>>::SELECT_LEN
        + 4
        + if INNER { 6 } else { 0 };
    /// Resolved descriptor component.
    fn write(output: &mut String) {
        super::__private::write_identifier(output, A::NAME);
        output.push(':');
        output.push_str(E::RESOURCE);
        output.push('!');
        output.push_str(E::HINT);
        if INNER {
            output.push_str("!inner");
        }
        output.push('(');
        <D::Record as Projection<E::Target>>::write_selection(output);
        output.push(')');
    }
}
/// Resolved selection descriptor contract.
#[doc(hidden)]
pub trait Embedded<R: Relation>: SelectionField<R> {
    /// Resolved descriptor component.
    type Edge: Relationship<Source = R>;
    /// Resolved descriptor component.
    type Child: Selection<Relation = <Self::Edge as Relationship>::Target>;
    /// Resolved descriptor component.
    type Alias: Alias;
    /// Resolved descriptor component.
    fn child(self) -> Self::Child;
}
impl<E: Relationship, D: Selection<Relation = E::Target>, A: Alias, const INNER: bool>
    Embedded<E::Source> for Child<E, D, A, INNER>
{
    /// Resolved descriptor component.
    type Edge = E;
    /// Resolved descriptor component.
    type Child = D;
    /// Resolved descriptor component.
    type Alias = A;
    /// Resolved descriptor component.
    fn child(self) -> D {
        self.child
    }
}

/// Predicate-only child descriptor; exposes no selected descendants.
pub struct Predicate<E>(PhantomData<fn() -> E>);
impl<E> Copy for Predicate<E> {}
impl<E> Clone for Predicate<E> {
    fn clone(&self) -> Self {
        *self
    }
}
/// Decoder identity for a predicate-only embed (never included in the parent record).
#[derive(serde::Deserialize)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "Serde must decode an empty JSON object, not the null accepted by a unit struct."
)]
pub struct PredicateRecord {}
impl<R: Relation> Projection<R> for PredicateRecord {
    const SELECT_LEN: usize = 0;
    fn write_selection(_: &mut String) {}
}
impl<E: Relationship> Selection for Predicate<E> {
    type Relation = E::Target;
    type Record = PredicateRecord;
}
/// Construct a predicate-only child descriptor.
#[must_use]
pub fn empty<E: Relationship>(_: EdgeToken<E>) -> Predicate<E> {
    Predicate(PhantomData)
}
