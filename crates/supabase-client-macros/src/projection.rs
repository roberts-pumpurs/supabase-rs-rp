//! Shared projection compilation after caller-specific field resolution.
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Generics, Ident};

use crate::Names;

pub struct DecodedField {
    pub name: Ident,
    pub ty: Tokens,
    pub key: Tokens,
    pub slot: Ident,
}

// Both adapters resolve fields before entering this emitter. In particular, local
// schema-key lookup stays in its constructor, never in generic record items.
pub fn decoder(
    names: &Names,
    rt: &Tokens,
    record: &Ident,
    generics: &Generics,
    fields: &[DecodedField],
    marker: Option<&Ident>,
    expecting: (&str, &str),
) -> Tokens {
    let (expecting, key_expecting) = expecting;
    let de = syn::Lifetime::new(
        &format!("'{}", names.ident("de")),
        proc_macro2::Span::mixed_site(),
    );
    let mut deserialize_generics = generics.clone();
    deserialize_generics
        .params
        .insert(0, syn::parse_quote!(#de));
    let (de_impl, _, _) = deserialize_generics.split_for_impl();
    let (params, args, bound) = generics.split_for_impl();
    let turbofish = args.as_turbofish();
    let deserializer_ident = names.ident("D");
    let error_ident = names.ident("E");
    let map_access_ident = names.ident("M");
    let key_type = names.ident("Key");
    let key_visitor = names.ident("KeyVisitor");
    let visitor = names.ident("Visitor");
    let key = names.ident("key");
    let map = names.ident("map");
    let field_names: Vec<_> = fields.iter().map(|field| &field.name).collect();
    let types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let keys: Vec<_> = fields.iter().map(|field| &field.key).collect();
    let slots: Vec<_> = fields.iter().map(|field| &field.slot).collect();
    let indices: Vec<_> = (0..fields.len()).collect();
    let identity = quote!(fn() -> #record #args);
    let values = fields.iter().map(|field| {
        let slot = &field.slot;
        let key = &field.key;
        quote!(#slot.ok_or_else(|| <#map_access_ident::Error as #rt::schema::__private::serde::de::Error>::missing_field(#key))?)
    }).collect::<Vec<_>>();
    let construct = if marker.is_some() {
        quote!(<#record #args>::new(#(#values),*))
    } else {
        quote!(#record { #(#field_names: #values,)* })
    };
    quote! {
        struct #key_type #params (::core::option::Option<::core::primitive::usize>, ::core::marker::PhantomData<#identity>) #bound;
        struct #key_visitor #params (::core::marker::PhantomData<#identity>) #bound;
        impl #de_impl #rt::schema::__private::serde::de::Visitor<#de> for #key_visitor #args #bound {
            type Value = #key_type #args;
            fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { f.write_str(#key_expecting) }
            fn visit_str<#error_ident: #rt::schema::__private::serde::de::Error>(self, #key: &::core::primitive::str) -> ::core::result::Result<Self::Value, #error_ident> {
                #(if #key == #keys { return ::core::result::Result::Ok(#key_type(::core::option::Option::Some(#indices), ::core::marker::PhantomData)); })*
                ::core::result::Result::Ok(#key_type(::core::option::Option::None, ::core::marker::PhantomData))
            }
        }
        impl #de_impl #rt::schema::__private::serde::Deserialize<#de> for #key_type #args #bound {
            fn deserialize<#deserializer_ident: #rt::schema::__private::serde::Deserializer<#de>>(deserializer_ident: #deserializer_ident) -> ::core::result::Result<Self, #deserializer_ident::Error> {
                deserializer_ident.deserialize_identifier(#key_visitor #turbofish (::core::marker::PhantomData))
            }
        }
        struct #visitor #params (::core::marker::PhantomData<#identity>) #bound;
        impl #de_impl #rt::schema::__private::serde::de::Visitor<#de> for #visitor #args #bound {
            type Value = #record #args;
            fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { f.write_str(#expecting) }
            fn visit_map<#map_access_ident: #rt::schema::__private::serde::de::MapAccess<#de>>(self, mut #map: #map_access_ident) -> ::core::result::Result<Self::Value, #map_access_ident::Error> {
                #(let mut #slots: ::core::option::Option<#types> = ::core::option::Option::None;)*
                while let ::core::option::Option::Some(#key) = #map.next_key::<#key_type #args>()? {
                    match #key.0 {
                        #(::core::option::Option::Some(#indices) => {
                            if #slots.is_some() { return ::core::result::Result::Err(<#map_access_ident::Error as #rt::schema::__private::serde::de::Error>::duplicate_field(#keys)); }
                            #slots = ::core::option::Option::Some(#map.next_value()?);
                        })*
                        _ => { let _: #rt::schema::__private::serde::de::IgnoredAny = #map.next_value()?; }
                    }
                }
                ::core::result::Result::Ok(#construct)
            }
        }
        impl #de_impl #rt::schema::__private::serde::Deserialize<#de> for #record #args #bound {
            fn deserialize<#deserializer_ident: #rt::schema::__private::serde::Deserializer<#de>>(deserializer_ident: #deserializer_ident) -> ::core::result::Result<Self, #deserializer_ident::Error> {
                deserializer_ident.deserialize_map(#visitor #turbofish (::core::marker::PhantomData))
            }
        }
    }
}

pub fn rendering(
    rt: &Tokens,
    record: &Ident,
    relation: &Tokens,
    generics: &Generics,
    fields: &[Tokens],
) -> Tokens {
    let (params, args, bound) = generics.split_for_impl();
    quote! {
        impl #params #rt::schema::Projection<#relation> for #record #args #bound {
            const SELECT_LEN: ::core::primitive::usize = {
                #rt::schema::__private::assert_distinct(&[#(<#fields as #rt::schema::selection::SelectionField<#relation>>::KEY),*]);
                (0usize #(+ <#fields as #rt::schema::selection::SelectionField<#relation>>::LEN + 1)*).saturating_sub(1)
            };
            fn write_selection(out: &mut ::std::string::String) {
                let start = out.len();
                #(if out.len() != start { out.push(','); }
                <#fields as #rt::schema::selection::SelectionField<#relation>>::write(out);)*
            }
        }
    }
}
