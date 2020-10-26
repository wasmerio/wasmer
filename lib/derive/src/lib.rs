extern crate proc_macro;

use proc_macro2::{Span, TokenStream};
use proc_macro_error::{abort, abort_call_site, proc_macro_error, set_dummy};
use quote::{format_ident, quote, quote_spanned, ToTokens};
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
    let (trait_methods, helper_methods) = derive_struct_fields(data);
    quote! {
        impl ::wasmer::WasmerEnv for #name {
            #trait_methods
        }

        impl #name {
            #helper_methods
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

fn derive_struct_fields(data: &DataStruct) -> (TokenStream, TokenStream) {
    let mut finish = vec![];
    let mut free = vec![];
    let mut helpers = vec![];
    //let mut assign_tokens = vec![];
    let mut touched_fields = vec![];
    match data.fields {
        Fields::Named(ref fields) => {
            for f in fields.named.iter() {
                let name = f.ident.as_ref().unwrap();
                let top_level_ty: &Type = &f.ty;
                dbg!(top_level_ty);
                touched_fields.push(name.clone());
                let mut wasmer_attr = None;
                for attr in &f.attrs {
                    // if / filter
                    let tokens = attr.tokens.clone();
                    wasmer_attr = Some(syn::parse2(tokens).unwrap());
                    break;
                }

                if let Some(wasmer_attr) = wasmer_attr {
                    let inner_type = get_identifier(top_level_ty);
                    let name_ref_str = format!("{}_ref", name);
                    let name_ref = syn::Ident::new(&name_ref_str, name.span());
                    let helper_tokens = quote_spanned! {f.span()=>
                        pub fn #name_ref(&self) -> &#inner_type {
                            unsafe { self.#name.get_unchecked() }
                        }
                    };
                    helpers.push(helper_tokens);
                    match wasmer_attr {
                        WasmerAttr::Export { identifier, ty } => match ty {
                            ExportAttr::Function {} => todo!(),
                            ExportAttr::Memory {} => {
                                let finish_tokens = quote_spanned! {f.span()=>
                                        let #name = instance.exports.get_memory(#identifier).unwrap();
                                        self.#name.initialize(#name.clone());
                                };
                                finish.push(finish_tokens);
                                let free_tokens = quote_spanned! {f.span()=>
                                };
                                free.push(free_tokens);
                            }
                        },
                    }
                }
            }
        }
        _ => todo!(),
    }

    let trait_methods = quote! {
        fn finish(&mut self, instance: &::wasmer::Instance) {
            #(#finish)*
        }

        fn free(&mut self) {
            unsafe {
                #(#free)*
            }
        }
    };

    let helper_methods = quote! {
        #(#helpers)*
    };

    (trait_methods, helper_methods)
}

// TODO: name this something that makes sense
fn get_identifier(ty: &Type) -> TokenStream {
    match ty {
        Type::Path(TypePath {
            path: Path { segments, .. },
            ..
        }) => {
            if let Some(PathSegment { ident, arguments }) = segments.last() {
                let ident_str = ident.to_string();
                if ident != "InitAfterInstance" {
                    // TODO:
                    panic!("Only the `InitAfterInstance` type is supported right now");
                }
                if let PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                    args, ..
                }) = arguments
                {
                    // TODO: proper error handling
                    assert_eq!(args.len(), 1);
                    if let GenericArgument::Type(Type::Path(TypePath {
                        path: Path { segments, .. },
                        ..
                    })) = &args[0]
                    {
                        segments
                            .last()
                            .expect("there must be at least one segment; TODO: error handling")
                            .to_token_stream()
                    } else {
                        panic!(
                            "unrecognized type in first generic position on `InitAfterInstance`"
                        );
                    }
                } else {
                    panic!("Expected a generic parameter on `InitAfterInstance`");
                }
            } else {
                panic!("Wrong type of type found");
            }
        }
        _ => todo!("Unrecognized/unsupported type"),
    }
}
