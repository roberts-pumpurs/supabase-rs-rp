//! Pure `PostgREST` query pairs; pass these directly to an HTTP query serializer.
//!
//! Runtime table names need no generated relation marker:
//! ```rust,ignore
//! let table = "articles";
//! let mut pairs = vec![
//!     params::select("*"),
//!     params::order_by("created_at", params::Order::Desc),
//! ];
//! pairs.extend(params::range(0, 24)?);
//! let response = http.get(format!("{base}/rest/v1/{table}"))
//!     .query(&pairs).send().await?;
//! ```
//! Values are unencoded; the HTTP serializer performs URL encoding exactly once.
use super::{Column, JsonColumn, NullableColumn, Projection, Relation};
use alloc::borrow::Cow;
use core::{
    borrow::Borrow,
    fmt::{self, Write as _},
};

/// An unencoded query key and value.
pub type QueryPair = (Cow<'static, str>, Cow<'static, str>);
/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Sort in ascending order.
    Asc,
    /// Sort in descending order.
    Desc,
}
/// Explicit SQL null placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nulls {
    /// Place null values before non-null values.
    First,
    /// Place null values after non-null values.
    Last,
}

/// An inclusive range that cannot be represented as an offset and `usize` limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// The upper bound precedes the lower bound.
    Reversed,
    /// The inclusive row count exceeds `usize::MAX`.
    Overflow,
}

impl fmt::Display for RangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Reversed => "inclusive range upper bound precedes lower bound",
            Self::Overflow => "inclusive range row count exceeds usize::MAX",
        })
    }
}

impl core::error::Error for RangeError {}

/// Render an unencoded runtime selection expression, such as `"*"` or `"id,name"`.
///
/// This accepts `PostgREST` selection grammar, not a literal identifier.
#[must_use]
pub fn select(selection: &str) -> QueryPair {
    (Cow::Borrowed("select"), Cow::Owned(selection.into()))
}

/// Render a response row limit. Zero requests zero rows.
#[must_use]
pub fn limit(count: usize) -> QueryPair {
    (Cow::Borrowed("limit"), Cow::Owned(count.to_string()))
}

/// Render a zero-based response row offset.
#[must_use]
pub fn offset(count: usize) -> QueryPair {
    (Cow::Borrowed("offset"), Cow::Owned(count.to_string()))
}

/// Render an inclusive response range as `[offset(low), limit(high - low + 1)]`.
///
/// # Errors
/// Returns [`RangeError::Reversed`] if `high < low`, or [`RangeError::Overflow`]
/// if the inclusive row count cannot fit in `usize`.
pub fn range(low: usize, high: usize) -> Result<[QueryPair; 2], RangeError> {
    let count = high
        .checked_sub(low)
        .ok_or(RangeError::Reversed)?
        .checked_add(1)
        .ok_or(RangeError::Overflow)?;
    Ok([offset(low), limit(count)])
}

/// Prefix a pair with a runtime embedded relation path.
///
/// Each path element is one literal selected relation name or alias, not dotted
/// grammar or a table URL. For example, `scope(&["tasks"], limit(5))` limits
/// embedded `tasks` rows, not root rows. Nested paths are escaped segment by
/// segment. An empty path leaves the pair unchanged. No generated marker is needed.
#[must_use]
pub fn scope(path: &[&str], pair: QueryPair) -> QueryPair {
    if path.is_empty() {
        return pair;
    }
    let mut key = String::new();
    for name in path {
        super::relationship::write_identifier(&mut key, name);
        key.push('.');
    }
    key.push_str(&pair.0);
    (Cow::Owned(key), pair.1)
}

/// Render a named selection.
#[must_use]
pub fn projection<R: Relation, P: Projection<R>>() -> QueryPair {
    (Cow::Borrowed("select"), P::selection())
}
macro_rules! comparison {
    ($($method:ident),*) => {$ (
        #[doc = concat!("Render a `", stringify!($method), "` scalar predicate.")]
        ///
        /// # Panics
        /// Panics if the scalar's `Display` implementation returns a formatting error.
        #[must_use]
        pub fn $method<C: Column, V: fmt::Display + ?Sized>(_column: C, value: &V) -> QueryPair
        where C::Filter: Borrow<V> {
            (Cow::Borrowed(C::SELECT), Cow::Owned(scalar_value(stringify!($method), value)))
        }
    )*};
}
comparison!(eq, neq, gt, gte, lt, lte);

fn scalar_value<V: fmt::Display + ?Sized>(operator: &str, value: &V) -> String {
    format!("{operator}.{value}")
}

/// Scalar comparison supported by runtime filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Equal.
    Eq,
    /// Not equal.
    Neq,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
}

impl Op {
    const fn name(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
        }
    }
}

/// Render a runtime literal column comparison. Values remain literal scalars.
///
/// # Panics
/// Panics if the value's `Display` implementation fails.
#[must_use]
pub fn filter<V: fmt::Display>(column: &str, op: Op, value: V) -> QueryPair {
    let mut key = String::with_capacity(super::relationship::identifier_len(column));
    super::relationship::write_identifier(&mut key, column);
    (Cow::Owned(key), Cow::Owned(scalar_value(op.name(), &value)))
}

/// A pair cannot safely be composed as a scalar predicate or checked group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionError;

impl fmt::Display for CompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected a scalar comparison or a checked nonempty logical group")
    }
}

impl core::error::Error for CompositionError {}

/// Compose comparisons or nested groups with OR.
///
/// Scalar values are quoted as literals, never parsed as logical grammar.
/// Nested `or`/`and` pairs must use the checked grammar emitted by these functions.
///
/// # Errors
/// Rejects empty groups, non-filter pairs, and malformed nested group grammar.
pub fn or(pairs: &[QueryPair]) -> Result<QueryPair, CompositionError> {
    compose("or", pairs)
}

/// Compose comparisons or nested groups with AND.
///
/// # Errors
/// Has the same checks and literal-value semantics as [`or`].
pub fn and(pairs: &[QueryPair]) -> Result<QueryPair, CompositionError> {
    compose("and", pairs)
}

fn scalar_operator(operator: &str) -> bool {
    matches!(
        operator,
        "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "like" | "ilike" | "is"
    )
}
fn is_operand(literal: &str) -> bool {
    matches!(literal, "null" | "true" | "false" | "unknown")
}

fn compose(kind: &'static str, pairs: &[QueryPair]) -> Result<QueryPair, CompositionError> {
    if pairs.is_empty() {
        return Err(CompositionError);
    }
    let mut output = String::from("(");
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        if key == "or" || key == "and" {
            let mut grammar = Grammar(value.as_bytes());
            grammar.group()?;
            if !grammar.0.is_empty() {
                return Err(CompositionError);
            }
            output.push_str(key);
            output.push_str(value);
        } else {
            let mut identifier = Grammar(key.as_bytes());
            identifier.identifier()?;
            if !identifier.0.is_empty() {
                return Err(CompositionError);
            }
            let (operator, literal) = value.split_once('.').ok_or(CompositionError)?;
            if !scalar_operator(operator) {
                return Err(CompositionError);
            }
            output.push_str(key);
            output.push('.');
            output.push_str(operator);
            output.push('.');
            if operator == "is" {
                if !is_operand(literal) {
                    return Err(CompositionError);
                }
                output.push_str(literal);
            } else {
                output.push('"');
                // Escaped only writes into a String and cannot return an error.
                Escaped(&mut output)
                    .write_str(literal)
                    .map_err(|_format_error| CompositionError)?;
                output.push('"');
            }
        }
    }
    output.push(')');
    Ok((Cow::Borrowed(kind), Cow::Owned(output)))
}

// Scalar values must be quoted. IS operands use a fixed whitelist of unquoted tokens.
struct Grammar<'a>(&'a [u8]);

#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "Successful prefix and delimiter checks bound every parser index below the slice length."
)]
impl Grammar<'_> {
    fn take(&mut self, byte: u8) -> Result<(), CompositionError> {
        if self.0.first() != Some(&byte) {
            return Err(CompositionError);
        }
        self.0 = &self.0[1..];
        Ok(())
    }

    fn quoted(&mut self) -> Result<(), CompositionError> {
        self.take(b'"')?;
        while let Some((&byte, rest)) = self.0.split_first() {
            self.0 = rest;
            match byte {
                b'"' => return Ok(()),
                b'\\' => {
                    let Some((&escaped, rest)) = self.0.split_first() else {
                        return Err(CompositionError);
                    };
                    if escaped != b'"' && escaped != b'\\' {
                        return Err(CompositionError);
                    }
                    self.0 = rest;
                }
                _ => {}
            }
        }
        Err(CompositionError)
    }

    fn identifier(&mut self) -> Result<(), CompositionError> {
        if self.0.first() == Some(&b'"') {
            return self.quoted();
        }
        let count = self
            .0
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count();
        if count == 0 {
            return Err(CompositionError);
        }
        self.0 = &self.0[count..];
        Ok(())
    }

    fn group(&mut self) -> Result<(), CompositionError> {
        self.take(b'(')?;
        loop {
            if self.0.starts_with(b"or(") {
                self.0 = &self.0[2..];
                self.group()?;
            } else if self.0.starts_with(b"and(") {
                self.0 = &self.0[3..];
                self.group()?;
            } else {
                self.identifier()?;
                self.take(b'.')?;
                let end = self
                    .0
                    .iter()
                    .position(|byte| *byte == b'.')
                    .ok_or(CompositionError)?;
                let operator =
                    core::str::from_utf8(&self.0[..end]).map_err(|_utf8_error| CompositionError)?;
                if !scalar_operator(operator) {
                    return Err(CompositionError);
                }
                self.0 = &self.0[end + 1..];
                if operator == "is" {
                    let end = self
                        .0
                        .iter()
                        .position(|byte| *byte == b',' || *byte == b')')
                        .ok_or(CompositionError)?;
                    let literal = core::str::from_utf8(&self.0[..end])
                        .map_err(|_utf8_error| CompositionError)?;
                    if !is_operand(literal) {
                        return Err(CompositionError);
                    }
                    self.0 = &self.0[end..];
                } else {
                    self.quoted()?;
                }
            }
            match self.0.first() {
                Some(b',') => self.0 = &self.0[1..],
                Some(b')') => return self.take(b')'),
                _ => return Err(CompositionError),
            }
        }
    }
}

/// Render a lexicographic ascending cursor predicate.
///
/// Order rows by every cursor column in the same order, ascending. Cursor values
/// must be non-null. Include a unique tie-breaker to avoid skipped or repeated rows.
/// An empty cursor returns `None`, so the first page needs no cursor predicate.
///
/// # Panics
/// Panics if a cursor value's `Display` implementation fails.
#[must_use]
pub fn after<V: fmt::Display>(cursor: &[(&str, V)]) -> Option<QueryPair> {
    if cursor.is_empty() {
        return None;
    }
    let mut output = String::from("(");
    for (index, (column, value)) in cursor.iter().enumerate() {
        if index != 0 {
            output.push_str(",and(");
        }
        for (prefix_column, prefix_value) in cursor.iter().take(index) {
            write_cursor_term(&mut output, prefix_column, Op::Eq, prefix_value);
            output.push(',');
        }
        write_cursor_term(&mut output, column, Op::Gt, value);
        if index != 0 {
            output.push(')');
        }
    }
    output.push(')');
    Some((Cow::Borrowed("or"), Cow::Owned(output)))
}

#[expect(
    clippy::expect_used,
    reason = "String writes cannot fail; scalar Display failures are documented."
)]
fn write_cursor_term<V: fmt::Display>(output: &mut String, column: &str, op: Op, value: &V) {
    super::relationship::write_identifier(output, column);
    output.push('.');
    output.push_str(op.name());
    output.push_str(".\"");
    write!(Escaped(&mut *output), "{value}").expect("scalar Display failed while rendering cursor");
    output.push('"');
}
/// Render a SQL null predicate.
#[must_use]
pub const fn is_null<C: NullableColumn>(_column: C) -> QueryPair {
    (Cow::Borrowed(C::SELECT), Cow::Borrowed("is.null"))
}
struct Escaped<'a>(&'a mut String);
impl fmt::Write for Escaped<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for character in value.chars() {
            if character == '"' || character == '\\' {
                self.0.push('\\');
            }
            self.0.push(character);
        }
        Ok(())
    }
}
/// Render literal IN values, quoting and escaping in list context only.
///
/// # Panics
/// Panics if a scalar's `Display` implementation returns a formatting error.
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "String writes cannot fail; retain the documented Display failure semantics."
)]
pub fn in_<'a, C, V, I>(_column: C, values: I) -> QueryPair
where
    C: Column,
    C::Filter: Borrow<V>,
    V: fmt::Display + ?Sized + 'a,
    I: IntoIterator<Item = &'a V>,
{
    let mut output = String::from("in.(");
    let mut first = true;
    for value in values {
        if !first {
            output.push(',');
        }
        first = false;
        output.push('"');
        // String writes cannot fail; retain Display's normal formatting failure semantics.
        write!(Escaped(&mut output), "{value}")
            .expect("scalar Display failed while rendering IN values");
        output.push('"');
    }
    output.push(')');
    (Cow::Borrowed(C::SELECT), Cow::Owned(output))
}
/// Render one ordering term.
#[must_use]
pub fn order<C: Column>(column: C, direction: Order) -> QueryPair {
    order_with_nulls_impl(column, direction, None)
}
/// Render one ordering term with explicit null placement.
#[must_use]
pub fn order_with_nulls<C: Column>(column: C, direction: Order, nulls: Nulls) -> QueryPair {
    order_with_nulls_impl(column, direction, Some(nulls))
}

/// Render ordering by one runtime literal column name, escaping grammar punctuation.
///
/// Expressions, dotted paths, and pre-escaped names are treated as literal names.
#[must_use]
pub fn order_by(column: &str, direction: Order) -> QueryPair {
    runtime_order(column, direction, None)
}

/// Render runtime literal column ordering with explicit null placement.
#[must_use]
pub fn order_by_with_nulls(column: &str, direction: Order, nulls: Nulls) -> QueryPair {
    runtime_order(column, direction, Some(nulls))
}

fn runtime_order(column: &str, direction: Order, nulls: Option<Nulls>) -> QueryPair {
    let (direction, nulls) = order_suffix(direction, nulls);
    let capacity = super::relationship::identifier_len(column)
        .saturating_add(direction.len())
        .saturating_add(nulls.len());
    let mut value = String::with_capacity(capacity);
    super::relationship::write_identifier(&mut value, column);
    value.push_str(direction);
    value.push_str(nulls);
    (Cow::Borrowed("order"), Cow::Owned(value))
}
fn order_with_nulls_impl<C: Column>(
    _column: C,
    direction: Order,
    nulls: Option<Nulls>,
) -> QueryPair {
    let (direction, nulls) = order_suffix(direction, nulls);
    (
        Cow::Borrowed("order"),
        Cow::Owned([C::SELECT, direction, nulls].concat()),
    )
}

const fn order_suffix(direction: Order, nulls: Option<Nulls>) -> (&'static str, &'static str) {
    let direction = match direction {
        Order::Asc => ".asc",
        Order::Desc => ".desc",
    };
    let nulls = match nulls {
        Some(Nulls::First) => ".nullsfirst",
        Some(Nulls::Last) => ".nullslast",
        None => "",
    };
    (direction, nulls)
}
/// Render equality on a JSON text path; path identifiers are escaped, values stay literal.
///
/// # Errors
/// Returns [`rp_postgrest::ConfigError::EmptyJsonPath`] when `path` is empty.
pub fn json_text_eq<C: JsonColumn>(
    _column: C,
    path: &[&str],
    value: &str,
) -> Result<QueryPair, rp_postgrest::Error> {
    let Some((last, parents)) = path.split_last() else {
        return Err(rp_postgrest::ConfigError::EmptyJsonPath.into());
    };
    let mut key = String::from(C::SELECT);
    for parent in parents {
        key.push_str("->");
        super::relationship::write_identifier(&mut key, parent);
    }
    key.push_str("->>");
    super::relationship::write_identifier(&mut key, last);
    Ok((Cow::Owned(key), Cow::Owned(scalar_value("eq", value))))
}
