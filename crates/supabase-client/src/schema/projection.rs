//! Named scalar and relationship projections.
/// Define a named result shape from generated columns and typed relationships.
#[macro_export]
macro_rules! projection {
    ($($input:tt)*) => {
        $crate::schema::__private::__projection! { [$crate] $($input)* }
    };
}

/// A projection's compile-time mapping of a shared filter key to a relation column.
/// Use `key!(type name)` to name a shared key in generic bounds.
pub trait FilterColumn<K, R: super::Relation> {
    /// Generated column for this exact relation.
    type Column: super::Column<Relation = R>;
}

/// Shared filter lookup for `projection!` key methods. `Name` is a unit struct named after the key,
/// so the diagnostic prints the key as a word. Implemented whenever `FilterColumn` resolves.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "shared filter `{Name}` of `{Self}` has no column on `{R}`",
    label = "`{R}` is not a relation of `{Self}`"
)]
pub trait SharedFilterFor<Name, R: super::Relation> {}
#[diagnostic::do_not_recommend]
impl<P: FilterColumn<N::Key, R>, N: super::KeyName, R: super::Relation> SharedFilterFor<N, R>
    for P
{
}

/// A zero-sized shared filter key scoped to a projection and relation.
#[expect(
    clippy::type_complexity,
    reason = "Function markers retain projection, key, and relation identity without ownership bounds."
)]
pub struct SharedFilter<P, K, R>(core::marker::PhantomData<fn() -> (P, K, R)>);
impl<P, K, R> Copy for SharedFilter<P, K, R> {}
impl<P, K, R> Clone for SharedFilter<P, K, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P, K, R> SharedFilter<P, K, R> {
    /// Construct the zero-sized marker emitted by `projection!`.
    #[doc(hidden)]
    #[must_use]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}
impl<P, K, R> Default for SharedFilter<P, K, R> {
    fn default() -> Self {
        Self::new()
    }
}
impl<P, K, R: super::Relation> super::Column for SharedFilter<P, K, R>
where
    P: FilterColumn<K, R>,
{
    type Relation = R;
    type Value = <P::Column as super::Column>::Value;
    type Filter = <P::Column as super::Column>::Filter;
    const NAME: &'static str = <P::Column as super::Column>::NAME;
    const SELECT: &'static str = <P::Column as super::Column>::SELECT;
}
impl<P, K, R: super::Relation> super::NullableColumn for SharedFilter<P, K, R>
where
    P: FilterColumn<K, R>,
    P::Column: super::NullableColumn,
{
}
impl<P, K, R: super::Relation> super::JsonColumn for SharedFilter<P, K, R>
where
    P: FilterColumn<K, R>,
    P::Column: super::JsonColumn,
{
}

/// Check shared filter types without requiring equal SQL names.
#[doc(hidden)]
pub const fn assert_same_filter<
    A: super::Column,
    B: super::Column<Value = A::Value, Filter = A::Filter>,
>() {
}
