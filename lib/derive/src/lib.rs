extern crate proc_macro;

use proc_macro2::{Span, TokenStream};
use proc_macro_error::{abort, abort_call_site, proc_macro_error, set_dummy};
use quote::{format_ident, quote, quote_spanned};
use syn::{punctuated::Punctuated, spanned::Spanned, token::Comma, *};

mod parse;

use crate::parse::{ExportAttr, WasmerAttr};

#[proc_macro_derive(WasmerEnv, attributes(wasmer))]
#[proc_macro_error]
pub fn derive_wasmer_env(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    let gen = impl_wasmer_env(&input);
    gen.into()
}

fn impl_wasmer_env_for_struct(
    name: &Ident,
    data: &DataStruct,
    _attrs: &[Attribute],
) -> TokenStream {
    let impl_inner = derive_struct_fields(data);
    quote! {
        impl ::wasmer::WasmerEnv for #name {
            #impl_inner
        }
    }
}

fn impl_wasmer_env(input: &DeriveInput) -> TokenStream {
    use syn::Data::*;
    let struct_name = &input.ident;

    set_dummy(quote! {
        impl ::wasmer::WasmerEnv for #struct_name {
            fn finish(&mut self, instance: &::wasmer::Instance) {
            }
            fn free(&mut self) {
            }
        }
    });

    match &input.data {
        Data::Struct(ds) => impl_wasmer_env_for_struct(struct_name, ds, &input.attrs),
        _ => todo!(),
    }
    /*match input.data {
        Struct(ds /*DataStruct {
            fields: syn::Fields::Named(ref fields),
            ..
        }*/) => ,
        Enum(ref e) => impl_wasmer_env_for_enum(struct_name, &e.variants, &input.attrs),
        _ => abort_call_site!("structopt only supports non-tuple structs and enums"),
    }*/
}

fn derive_struct_fields(data: &DataStruct) -> TokenStream {
    let mut finish = vec![];
    let mut free = vec![];
    let mut assign_tokens = vec![];
    let mut touched_fields = vec![];
    match data.fields {
        Fields::Named(ref fields) => {
            for f in fields.named.iter() {
                let name = f.ident.as_ref().unwrap();
                touched_fields.push(name.clone());
                let mut wasmer_attr = None;
                for attr in &f.attrs {
                    // if / filter
                    let tokens = attr.tokens.clone();
                    wasmer_attr = Some(syn::parse2(tokens).unwrap());
                    break;
                }

                if let Some(wasmer_attr) = wasmer_attr {
                    match wasmer_attr {
                        WasmerAttr::Export { identifier, ty } => match ty {
                            ExportAttr::Function {} => todo!(),
                            ExportAttr::Memory {} => {
                                let finish_tokens = quote_spanned! {f.span()=>
                                        let #name = instance.exports.get_memory(#identifier).unwrap();
                                        let #name = Box::into_raw(Box::new(#name.clone()));
                                };
                                finish.push(finish_tokens);
                                let free_tokens = quote_spanned! {f.span()=>
                                    let _ = Box::from_raw(self.#name);
                                    self.#name = ::std::ptr::null_mut();
                                };
                                free.push(free_tokens);
                            }
                        },
                    }
                    assign_tokens.push(quote! {
                        self.#name = #name;
                    });
                }
            }
        }
        _ => todo!(),
    }

    quote! {
        fn finish(&mut self, instance: &::wasmer::Instance) {
            #(#finish)*
            #(#assign_tokens)*
        }

        fn free(&mut self) {
            unsafe {
                #(#free)*
            }
        }
    }
}
