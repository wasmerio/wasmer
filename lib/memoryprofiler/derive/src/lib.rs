extern crate proc_macro;
use quote::{quote, quote_spanned};
use syn::*;

#[proc_macro_derive(MemoryUsage)]
pub fn derive_memory_usage(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: DeriveInput = parse(input).unwrap();
    match input.data {
        Data::Struct(ref struct_data) => {
            derive_memory_usage_struct(&input.ident, struct_data, &input.generics)
        }
        Data::Enum(ref enum_data) => {
            derive_memory_usage_enum(&input.ident, enum_data, &input.generics)
        }
        Data::Union(_) => panic!("unions are not yet implemented"),
        /*
        // TODO: unions.
        // We have no way of knowing which union member is active, so we should
        // refuse to derive an impl except for unions where all members are
        // primitive types or arrays of them.
        Data::Union(ref union_data) => {
            derive_memory_usage_union(union_data)
        },
        */
    }
}

// TODO: use Iterator::fold_first once it's stable. https://github.com/rust-lang/rust/pull/79805
fn join_fold<I, F, B>(mut iter: I, function: F, empty: B) -> B
where
    I: Iterator<Item = B>,
    F: FnMut(B, I::Item) -> B,
{
    if let Some(first) = iter.next() {
        iter.fold(first, function)
    } else {
        empty
    }
}

fn derive_memory_usage_struct(
    struct_name: &Ident,
    data: &DataStruct,
    generics: &Generics,
) -> proc_macro::TokenStream {
    let lifetimes_and_generics = &generics.params;
    let where_clause = &generics.where_clause;
    let sum = join_fold(
        match &data.fields {
            Fields::Named(ref fields) => fields
                .named
                .iter()
                .map(|field| {
                    let id = field.ident.as_ref().unwrap();
                    let span = id.span();
                    quote_spanned! ( span=>MemoryUsage::size_of_val(&self.#id) - std::mem::size_of_val(&self.#id) )
                })
                .collect(),
            Fields::Unit => vec![],
            Fields::Unnamed(ref fields) => (0..(fields.unnamed.iter().count()))
                .into_iter()
                .map(|field| {
                    let id = Index::from(field);
                    quote! { MemoryUsage::size_of_val(&self.#id) - std::mem::size_of_val(&self.#id) }
                })
                .collect(),
        }
        .iter()
        .cloned(), // TODO: shouldn't need cloned here
        |x, y| quote! { #x + #y },
        quote! { 0 },
    );

    (quote! {
        #[allow(dead_code)]
        impl < #lifetimes_and_generics > MemoryUsage for #struct_name < #lifetimes_and_generics > #where_clause {
            fn size_of_val(&self) -> usize {
                std::mem::size_of_val(self) + #sum
            }
        }
    })
    .into()
}

fn derive_memory_usage_enum(
    struct_name: &Ident,
    data: &DataEnum,
    generics: &Generics,
) -> proc_macro::TokenStream {
    let lifetimes_and_generics = &generics.params;
    let where_clause = &generics.where_clause;
    let each_variant = join_fold(
        data.variants.iter().map(|variant| {
            let ident = &variant.ident;
            let span = ident.span();
            let (pattern, sum) = match variant.fields {
                Fields::Named(ref fields) => {
                    let identifiers = fields.named.iter().map(|field| {
                        let id = field.ident.as_ref().unwrap();
                        let span = id.span();
                        quote_spanned!(span=>#id)
                    });
                    let pattern =
                        join_fold(identifiers.clone(), |x, y| quote! { #x , #y }, quote! {});
                    let sum = join_fold(
                        identifiers.map(|v| quote! { MemoryUsage::size_of_val(#v) - std::mem::size_of_val(#v) }),
                        |x, y| quote! { #x + #y },
                        quote! { 0 },
                    );
                    (quote! { { #pattern } }, quote! { #sum })
                }
                Fields::Unit => (quote! {}, quote! { 0 }),
                Fields::Unnamed(ref fields) => {
                    let identifiers =
                        (0..(fields.unnamed.iter().count()))
                            .into_iter()
                            .map(|field| {
                                let id = Ident::new(
                                    &format!("value{}", field),
                                    export::Span::call_site(),
                                );
                                quote!(#id)
                            });
                    let pattern =
                        join_fold(identifiers.clone(), |x, y| quote! { #x , #y }, quote! {});
                    let sum = join_fold(
                        identifiers.map(|v| quote! { MemoryUsage::size_of_val(#v) - std::mem::size_of_val(#v) }),
                        |x, y| quote! { #x + #y },
                        quote! { 0 },
                    );
                    (quote! { ( #pattern ) }, quote! { #sum })
                }
            };
            quote_spanned! { span=>Self::#ident#pattern => #sum }
        }),
        |x, y| quote! { #x , #y },
        quote! {},
    );
    //dbg!(&each_variant);

    (quote! {
        #[allow(dead_code)]
        impl < #lifetimes_and_generics > MemoryUsage for #struct_name < #lifetimes_and_generics > #where_clause {
            fn size_of_val(&self) -> usize {
                std::mem::size_of_val(self) + match self {
                    #each_variant
                }
            }
        }
    })
    .into()
}
