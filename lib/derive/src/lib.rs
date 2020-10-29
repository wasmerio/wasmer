extern crate proc_macro;

use proc_macro2::{Span, TokenStream};
use proc_macro_error::{abort, abort_call_site, proc_macro_error, set_dummy};
use quote::{quote, quote_spanned, ToTokens};
use syn::{spanned::Spanned, token::Comma, *};

mod parse;

use crate::parse::WasmerAttr;

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
    generics: &Generics,
    _attrs: &[Attribute],
) -> TokenStream {
    let (trait_methods, helper_methods) = derive_struct_fields(data);
    let lifetimes_and_generics = generics.params.clone();
    let where_clause = generics.where_clause.clone();
    quote! {
        impl < #lifetimes_and_generics > ::wasmer::WasmerEnv for #name < #lifetimes_and_generics > #where_clause{
            #trait_methods
        }

        impl < #lifetimes_and_generics > #name < #lifetimes_and_generics > #where_clause {
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
        Data::Struct(ds) => {
            impl_wasmer_env_for_struct(struct_name, ds, &input.generics, &input.attrs)
        }
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
                let name_str = name.to_string();
                let top_level_ty: &Type = &f.ty;
                //dbg!(top_level_ty);
                touched_fields.push(name.clone());
                let mut wasmer_attr = None;
                for attr in &f.attrs {
                    // if / filter
                    let tokens = attr.tokens.clone();
                    if let Ok(attr) = syn::parse2(tokens) {
                        wasmer_attr = Some(attr);
                        break;
                    }
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
                        WasmerAttr::Export { identifier, .. } => {
                            let item_name =
                                identifier.unwrap_or_else(|| LitStr::new(&name_str, name.span()));
                            /*match ty {
                            ExportAttr::NativeFunc {} => {
                                let finish_tokens = quote_spanned! {f.span()=>
                                        let #name: #inner_type = instance.exports.get_native_function(#item_name).unwrap();
                                        self.#name.initialize(#name);
                                };
                                finish.push(finish_tokens);
                                let free_tokens = quote_spanned! {f.span()=>
                                };
                                free.push(free_tokens);
                            }
                            ExportAttr::EverythingElse {} => {*/
                            let finish_tokens = quote_spanned! {f.span()=>
                                    let #name: #inner_type = instance.exports.get_with_generics(#item_name)?;
                                    self.#name.initialize(#name);
                            };
                            finish.push(finish_tokens);
                            let free_tokens = quote_spanned! {f.span()=>
                            };
                            free.push(free_tokens);
                            //}
                            //}
                        }
                    }
                }
            }
        }
        _ => todo!(),
    }

    let trait_methods = quote! {
        fn finish(&mut self, instance: &::wasmer::Instance) -> Result<(), ::wasmer::HostEnvInitError> {
            #(#finish)*
            Ok(())
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
                if ident != "LazyInit" {
                    // TODO:
                    panic!("Only the `LazyInit` type is supported right now");
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
                        panic!("unrecognized type in first generic position on `LazyInit`");
                    }
                } else {
                    panic!("Expected a generic parameter on `LazyInit`");
                }
            } else {
                panic!("Wrong type of type found");
            }
        }
        _ => todo!("Unrecognized/unsupported type"),
    }
}
