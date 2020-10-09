//! Macros of rsfbclient

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields};

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
        use rsfbclient::{IntoParams, IntoParam, ParamsType};
        use std::collections::HashMap;

        impl IntoParams for #st_name {
            fn to_params(self) -> ParamsType {
                let mut params = HashMap::new();

                #(params.insert(#st_fields_params));*;

                ParamsType::Named(params)
            }
        }
    };

    TokenStream::from(st_impl)
}
