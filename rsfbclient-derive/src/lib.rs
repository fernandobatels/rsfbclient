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
        .map(|field| field.ident.as_ref().expect("Field name required"));
    let st_fields_names = st_fields_params.clone().map(|field| field.to_string());

    let st_impl = quote! {
        use rsfbclient_core::{IntoParams, IntoParam};
        impl IntoParams for #st_name {
            fn to_params(self) -> Vec<SqlType> {
                vec![
                    #(self.#st_fields_params.into_param()),*
                ]
            }

            fn names(&self) -> Option<Vec<String>> {
                Some(vec![
                    #(#st_fields_names.to_string()),*
                ])
            }
        }
    };

    TokenStream::from(st_impl)
}
