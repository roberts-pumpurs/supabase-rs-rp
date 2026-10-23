#![cfg_attr(doc, doc = include_str!("../README.md"))]
use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{quote, quote_spanned};
use syn::{
    Ident, Path, Token, Type, braced,
    ext::IdentExt as _,
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

mod named;
mod projection;

/// Version-owned support for the runtime's `$crate`-preserving `projection!` adapter.
#[doc(hidden)]
#[proc_macro]
pub fn __projection(input: TokenStream) -> TokenStream {
    let names = Names::new(input.clone().into());
    let input = parse_macro_input!(input as named::Input);
    named::expand(&names, &input).into()
}

struct Input {
    runtime: Path,
    root: Type,
    fields: Vec<Field>,
}
struct Field {
    alias: Ident,
    kind: Kind,
}
enum Kind {
    Scalar,
    Nested(Ident, Vec<Field>, bool),
    Shared(Ident, Box<Type>, bool),
    Empty(Ident, bool),
}
fn fields(input: ParseStream) -> syn::Result<Vec<Field>> {
    let body;
    braced!(body in input);
    let mut out = Vec::<Field>::new();
    while !body.is_empty() {
        let alias: Ident = body.parse()?;
        let normalized = alias.to_string().trim_start_matches("r#").to_owned();
        if out
            .iter()
            .any(|field| field.alias.to_string().trim_start_matches("r#") == normalized)
        {
            return Err(syn::Error::new(alias.span(), "duplicate selected alias"));
        }
        let kind = if body.peek(Token![:]) {
            body.parse::<Token![:]>()?;
            let edge: Ident = body.parse()?;
            if edge == "embed" || edge == "empty" || edge == "inner" {
                let args;
                parenthesized!(args in body);
                let key = args.parse()?;
                if edge == "inner" {
                    if !args.is_empty() {
                        return Err(args.error("expected inner(relationship)"));
                    }
                    Kind::Nested(key, fields(&body)?, true)
                } else {
                    let dto = if edge == "embed" {
                        args.parse::<Token![,]>()?;
                        Some(args.parse::<Type>()?)
                    } else {
                        None
                    };
                    let inner = if args.peek(Token![,]) {
                        args.parse::<Token![,]>()?;
                        let flag: Ident = args.parse()?;
                        if flag != "inner" {
                            return Err(syn::Error::new(flag.span(), "expected inner"));
                        }
                        true
                    } else {
                        false
                    };
                    if !args.is_empty() {
                        return Err(args.error("unexpected embed arguments"));
                    }
                    match dto {
                        Some(dto) => Kind::Shared(key, Box::new(dto), inner),
                        None => Kind::Empty(key, inner),
                    }
                }
            } else {
                Kind::Nested(edge, fields(&body)?, false)
            }
        } else {
            Kind::Scalar
        };
        out.push(Field { alias, kind });
        if body.is_empty() {
            break;
        }
        body.parse::<Token![,]>()?;
    }
    if out.is_empty() {
        return Err(body.error("select at least one field"));
    }
    Ok(out)
}
impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let runtime = if input.peek(Ident) && input.peek2(Token![=]) && !input.peek2(Token![=>]) {
            let keyword: Ident = input.parse()?;
            if keyword != "runtime" {
                return Err(syn::Error::new(
                    keyword.span(),
                    "expected runtime = crate_path;",
                ));
            }
            input.parse::<Token![=]>()?;
            let path = input.parse()?;
            input.parse::<Token![;]>()?;
            path
        } else {
            syn::parse_quote!(::rp_supabase_client)
        };
        let root = input.parse()?;
        input.parse::<Token![=>]>()?;
        let fields = fields(input)?;
        Ok(Self {
            runtime,
            root,
            fields,
        })
    }
}
/// Construct a typed query-local selection from a relation and selected fields.
#[proc_macro]
pub fn select(input: TokenStream) -> TokenStream {
    let names = Names::new(input.clone().into());
    let Input {
        runtime,
        root,
        fields,
    } = parse_macro_input!(input as Input);
    node(
        &names,
        &runtime,
        &fields,
        &quote!(#runtime::schema::selection::RelationToken::<#root>::new()),
    )
    .into()
}
// Every generated spelling is outside the entire caller token identifier set.
// Growing a prefix (rather than hashing) also covers nested paths and generic arguments.
struct Names {
    prefix: String,
}

impl Names {
    fn new(tokens: Tokens) -> Self {
        fn collect(tokens: Tokens, names: &mut Vec<String>) {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Ident(ident) => names.push(ident.unraw().to_string()),
                    proc_macro2::TokenTree::Group(group) => collect(group.stream(), names),
                    proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
                }
            }
        }
        let mut identifiers = Vec::new();
        collect(tokens, &mut identifiers);
        let mut prefix = String::from("__selection_");
        while identifiers.iter().any(|name| name.starts_with(&prefix)) {
            prefix.push('_');
        }
        Self { prefix }
    }

    fn ident(&self, suffix: &str) -> Ident {
        self.ident_at(suffix, proc_macro2::Span::mixed_site())
    }

    fn ident_at(&self, suffix: &str, span: proc_macro2::Span) -> Ident {
        Ident::new(&format!("{}{suffix}", self.prefix), span)
    }
}

struct EmissionField<'a> {
    field: &'a Field,
    index: usize,
    ty: Ident,
    argument: Ident,
    slot: Ident,
}

struct Emission<'a> {
    names: &'a Names,
    runtime: &'a Path,
    fields: Vec<EmissionField<'a>>,
    marker: Ident,
}

impl<'a> Emission<'a> {
    fn new(names: &'a Names, runtime: &'a Path, fields: &'a [Field]) -> Self {
        let marker = names.ident("marker");
        let fields = fields
            .iter()
            .enumerate()
            .map(|(index, field)| EmissionField {
                field,
                index,
                ty: names.ident(&format!("F{index}")),
                argument: names.ident(&format!("arg{index}")),
                slot: names.ident(&format!("slot{index}")),
            })
            .collect();
        Self {
            names,
            runtime,
            fields,
            marker,
        }
    }

    fn types(&self) -> impl Iterator<Item = &Ident> {
        self.fields.iter().map(|field| &field.ty)
    }

    fn decoded(&self) -> impl Iterator<Item = &EmissionField<'a>> {
        self.fields
            .iter()
            .filter(|field| !matches!(field.field.kind, Kind::Empty(..)))
    }

    fn embedded(&self) -> impl Iterator<Item = &EmissionField<'a>> {
        self.fields
            .iter()
            .filter(|field| !matches!(field.field.kind, Kind::Scalar))
    }

    fn bound(&self) -> Tokens {
        let r_ident = self.names.ident("R");
        let rt = self.runtime;
        let types = self.types();
        quote!(#r_ident: #rt::schema::Relation + #rt::schema::Projection<#r_ident>, #(#types: #rt::schema::selection::SelectionField<#r_ident>),*)
    }
}

fn node(names: &Names, rt: &Path, fields: &[Field], relation: &Tokens) -> Tokens {
    let construct_ident = names.ident("construct");
    let relation_ident = names.ident("relation");
    let emission = Emission::new(names, rt, fields);
    let record = record(&emission);
    let decoder = local_decoder(&emission);
    let projection = projection(&emission);
    let descriptor = descriptor(&emission);
    let values = emission
        .fields
        .iter()
        .map(|field| field_value(names, rt, field));
    quote!({
        #record
        #decoder
        #projection
        #descriptor
        let #relation_ident = #relation;
        #construct_ident(#relation_ident, #(#values),*)
    })
}

fn field_value(names: &Names, rt: &Path, field: &EmissionField<'_>) -> Tokens {
    let edge_ident = names.ident("edge");
    let alias = &field.field.alias;
    match &field.field.kind {
        Kind::Scalar => lookup(names, rt, "column", alias),
        Kind::Nested(edge, child, inner) => {
            let child = node(names, rt, child, &quote!(#edge_ident.target()));
            embedded_value(names, rt, field, edge, *inner, &child)
        }
        Kind::Shared(edge, dto, inner) => {
            let child = quote_spanned!(alias.span()=> #rt::schema::selection::shared::<_, #dto>(#edge_ident));
            embedded_value(names, rt, field, edge, *inner, &child)
        }
        Kind::Empty(edge, inner) => {
            let child = quote!(#rt::schema::selection::empty(#edge_ident));
            embedded_value(names, rt, field, edge, *inner, &child)
        }
    }
}

fn embedded_value(
    names: &Names,
    rt: &Path,
    field: &EmissionField<'_>,
    edge: &Ident,
    inner: bool,
    child: &Tokens,
) -> Tokens {
    let child_ident = names.ident("child");
    let edge_ident = names.ident("edge");
    let alias = &field.field.alias;
    let index = field.index;
    let alias_ty = names.ident(&format!("Alias{index}"));
    let alias_name = alias.unraw().to_string();
    let lookup = lookup(names, rt, "edge", edge);
    quote!({
        struct #alias_ty;
        impl #rt::schema::selection::Alias for #alias_ty { const NAME: &'static ::core::primitive::str = #alias_name; }
        let #edge_ident = #lookup;
        let #child_ident = #child;
        #rt::schema::selection::embed::<_, _, _, #inner>(#edge_ident, #child_ident, #alias_ty)
    })
}

// Generic items know only resolved Field descriptors; lookup constraints never enter decoder/rendering.
#[expect(
    clippy::cognitive_complexity,
    reason = "quote emits record trait implementations; the emitter has no branches"
)]
fn record(emission: &Emission<'_>) -> Tokens {
    let r_ident = emission.names.ident("R");
    let record_ident = emission.names.ident("Record");
    let s_ident = emission.names.ident("S");
    let rt = emission.runtime;
    let fs: Vec<_> = emission.types().collect();
    let bound = emission.bound();
    let marker = &emission.marker;
    let dfs: Vec<_> = emission.decoded().map(|field| &field.ty).collect();
    let dnames: Vec<_> = emission.decoded().map(|field| &field.field.alias).collect();
    let count = dfs.len();
    quote! {
        struct #record_ident<#r_ident,#(#fs),*> where #bound {
            #(pub #dnames: <#dfs as #rt::schema::selection::SelectionField<#r_ident>>::Value,)*
            #marker: ::core::marker::PhantomData<fn() -> (#r_ident,#(#fs),*)>,
        }
        impl<#r_ident,#(#fs),*> ::core::fmt::Debug for #record_ident<#r_ident,#(#fs),*> where #bound, #(<#dfs as #rt::schema::selection::SelectionField<#r_ident>>::Value: ::core::fmt::Debug,)* {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut d = f.debug_struct("Record"); #(d.field(::core::stringify!(#dnames), &self.#dnames);)* d.finish()
            }
        }
        impl<#r_ident,#(#fs),*> #rt::schema::__private::serde::Serialize for #record_ident<#r_ident,#(#fs),*> where #bound, #(<#dfs as #rt::schema::selection::SelectionField<#r_ident>>::Value: #rt::schema::__private::serde::Serialize,)* {
            fn serialize<#s_ident: #rt::schema::__private::serde::Serializer>(&self, s: #s_ident) -> ::core::result::Result<#s_ident::Ok,#s_ident::Error> {
                let mut m = s.serialize_map(::core::option::Option::Some(#count))?;
                #(#rt::schema::__private::serde::ser::SerializeMap::serialize_entry(&mut m, <#dfs as #rt::schema::selection::SelectionField<#r_ident>>::KEY, &self.#dnames)?;)* #rt::schema::__private::serde::ser::SerializeMap::end(m)
            }
        }
    }
}

fn local_generics(emission: &Emission<'_>) -> syn::Generics {
    let relation_ident = emission.names.ident("R");
    let fields = emission.types();
    let bound = emission.bound();
    let mut generics: syn::Generics = syn::parse_quote!(<#relation_ident, #(#fields),*>);
    generics.where_clause = Some(syn::parse_quote!(where #bound));
    generics
}

fn local_decoder(emission: &Emission<'_>) -> Tokens {
    let rt = emission.runtime;
    let relation_ident = emission.names.ident("R");
    let record = emission.names.ident("Record");
    let fields: Vec<_> = emission
        .decoded()
        .map(|field| {
            let ty = &field.ty;
            projection::DecodedField {
                name: field.field.alias.clone(),
                ty: quote!(<#ty as #rt::schema::selection::SelectionField<#relation_ident>>::Value),
                key: quote!(<#ty as #rt::schema::selection::SelectionField<#relation_ident>>::KEY),
                slot: field.slot.clone(),
            }
        })
        .collect();
    projection::decoder(
        emission.names,
        &quote!(#rt),
        &record,
        &local_generics(emission),
        &fields,
        Some(&emission.marker),
        ("a selected object", "a selected object field"),
    )
}

fn projection(emission: &Emission<'_>) -> Tokens {
    let rt = emission.runtime;
    let relation_ident = emission.names.ident("R");
    let record = emission.names.ident("Record");
    let fields = emission.types().map(|ty| quote!(#ty)).collect::<Vec<_>>();
    projection::rendering(
        &quote!(#rt),
        &record,
        &quote!(#relation_ident),
        &local_generics(emission),
        &fields,
    )
}

#[expect(
    clippy::cognitive_complexity,
    reason = "quote emits descriptor trait bounds; the emitter has no branches"
)]
fn descriptor(emission: &Emission<'_>) -> Tokens {
    let descriptor_ident = emission.names.ident("Descriptor");
    let r_ident = emission.names.ident("R");
    let record_ident = emission.names.ident("Record");
    let construct_ident = emission.names.ident("construct");
    let rt = emission.runtime;
    let fs: Vec<_> = emission.types().collect();
    let args: Vec<_> = emission
        .fields
        .iter()
        .map(|field| &field.argument)
        .collect();
    let bound = emission.bound();
    let marker = &emission.marker;
    let ebounds: Vec<_> = emission
        .embedded()
        .map(|field| {
            let ty = &field.ty;
            quote!(#ty: #rt::schema::selection::Embedded<#r_ident>)
        })
        .collect();
    let descfields: Vec<_> = emission.embedded().map(|field| {
        let ty = &field.ty;
        let alias = &field.field.alias;
        quote!(pub #alias: #rt::schema::selection::Handle<#record_ident<#r_ident,#(#fs),*>, <<#ty as #rt::schema::selection::Embedded<#r_ident>>::Child as #rt::schema::selection::Selection>::Record, <#ty as #rt::schema::selection::Embedded<#r_ident>>::Edge, <#ty as #rt::schema::selection::Embedded<#r_ident>>::Alias, <#ty as #rt::schema::selection::Embedded<#r_ident>>::Child>)
    }).collect();
    let descinit: Vec<_> = emission.embedded().map(|field| {
        let argument = &field.argument;
        let alias = &field.field.alias;
        quote!(#alias: #rt::schema::selection::Handle::new(#rt::schema::selection::Embedded::child(#argument)))
    }).collect();
    quote! {
        struct #descriptor_ident<#r_ident,#(#fs),*> where #bound, #(#ebounds,)* {
            #(#descfields,)*
            #marker: ::core::marker::PhantomData<fn() -> (#r_ident,#(#fs),*)>,
        }
        impl<#r_ident,#(#fs),*> ::core::marker::Copy for #descriptor_ident<#r_ident,#(#fs),*> where #bound, #(#ebounds,)* {}
        impl<#r_ident,#(#fs),*> ::core::clone::Clone for #descriptor_ident<#r_ident,#(#fs),*> where #bound, #(#ebounds,)* { fn clone(&self) -> Self { *self } }
        impl<#r_ident,#(#fs),*> #rt::schema::selection::Selection for #descriptor_ident<#r_ident,#(#fs),*> where #bound, #(#ebounds,)* {
            type Relation = #r_ident;
            type Record = #record_ident<#r_ident,#(#fs),*>;
        }
        fn #construct_ident<#r_ident,#(#fs),*>(_: #rt::schema::selection::RelationToken<#r_ident>, #(#args: #fs),*) -> #descriptor_ident<#r_ident,#(#fs),*> where #bound, #(#ebounds,)* {
            #descriptor_ident { #(#descinit,)* #marker: ::core::marker::PhantomData }
        }
    }
}

fn key_type(rt: &Path, name: &Ident) -> Tokens {
    let text = name.unraw().to_string();
    let characters = text.chars();
    quote!((#(#rt::schema::Character<#characters>,)*))
}
// Resolve `name` on the relation in scope with `selection::column` or `selection::edge`.
// The argument is a block-local unit struct named after the identifier the user wrote:
// lookup diagnostics print that type, so an unknown name appears as a word, not a key.
// Every token is located at `name`, so rustc points at the name, not the whole macro call.
// Only prefixed identifiers are in scope inside the struct block, so it shadows nothing.
fn lookup(names: &Names, rt: &Path, function: &str, name: &Ident) -> Tokens {
    let span = proc_macro2::Span::mixed_site().located_at(name.span());
    let function = Ident::new(function, span);
    let call = names.ident_at("lookup", span);
    let key_name = names.ident_at("KeyName", span);
    let key = names.ident_at("Key", span);
    let relation = names.ident_at("relation", span);
    let key_type = key_type(rt, name);
    let mut local = name.clone();
    local.set_span(span);
    quote_spanned!(span=> {
        use #rt::schema::{selection::#function as #call, KeyName as #key_name};
        type #key = #key_type;
        #call(#relation, {
            #[allow(non_camel_case_types)]
            struct #local;
            impl #key_name for #local { type Key = #key; }
            #local
        })
    })
}
struct KeyInput {
    runtime: Path,
    name: Ident,
    ty: bool,
}
impl Parse for KeyInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let runtime = if input.peek(Ident) && input.peek2(Token![=]) {
            let word: Ident = input.parse()?;
            if word != "runtime" {
                return Err(syn::Error::new(word.span(), "expected runtime"));
            }
            input.parse::<Token![=]>()?;
            let path = input.parse()?;
            input.parse::<Token![;]>()?;
            path
        } else {
            syn::parse_quote!(::rp_supabase_client)
        };
        let ty = input.peek(Token![type]);
        if ty {
            input.parse::<Token![type]>()?;
        }
        let name = input.parse()?;
        Ok(Self { runtime, name, ty })
    }
}
/// Construct a lossless schema key, or its name type with `key!(type identifier)`.
#[proc_macro]
pub fn key(input: TokenStream) -> TokenStream {
    let KeyInput { runtime, name, ty } = parse_macro_input!(input as KeyInput);
    let key = key_type(&runtime, &name);
    if ty {
        key.into()
    } else {
        quote!(#runtime::schema::Key::<#key>::new()).into()
    }
}
