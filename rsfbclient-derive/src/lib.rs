//! Macros of rsfbclient

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields};

/// Derive an [IntoParams<T>](../trait.IntoParams.html) implementation for structs.
///
/// This enables passing an instance of such a struct in places where named parameters
/// are expected, using the field labels to associate field values with parameter names.
///
/// The fields' types must implement the [IntoParam<T>](../trait.IntoParam.html) trait.
///
/// Note that `Option<T>` may be used as a field type to indicate a nullable parameter.
///
/// Providing an instance of the struct with value `None` for such a field corresponds to
/// passing a `null` value for that field.
#[proc_macro_derive(IntoParams)]
pub fn into_params_derive(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    let st_name = &input.ident;
    let st_fields = match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => &fields.named,
        _ => panic!("expected a struct with named fields"),
    };
    let st_fields_params = st_fields
        .iter()
        .map(|field| field.ident.as_ref().expect("Field name required"))
        .map(|field| {
            let field_str = field.to_string();
            quote! { #field_str.to_string(), self.#field.into_param() }
        });

    let st_impl = quote! {
        impl rsfbclient::IntoParams for #st_name {
            fn to_params(self) -> rsfbclient::ParamsType {
                use std::collections::HashMap;
                use rsfbclient::{IntoParams, IntoParam, ParamsType};

                let mut params = HashMap::new();

                #(params.insert(#st_fields_params));*;

                ParamsType::Named(params)
            }
        }
    };

    TokenStream::from(st_impl)
}
