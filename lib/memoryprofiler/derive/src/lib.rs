extern crate proc_macro;
use quote::quote;
use syn::*;

#[proc_macro_derive(MemoryUsage)]
pub fn derive_memory_usage(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    match input.data {
        Data::Struct(ref struct_data) => {
            derive_memory_usage_struct(&input.ident, struct_data, &input.generics)
        }
        _ => unreachable!("not yet implemented"),
        /*
        Data::Enum(ref enum_data) => {
            derive_memory_usage_struct(enum_data)
        },
        Data::Union(ref union_data) => {
            derive_memory_usage_union(union_data)
        },
        */
    }
}

fn derive_memory_usage_struct(
    struct_name: &Ident,
    data: &DataStruct,
    generics: &Generics,
) -> proc_macro::TokenStream {
    let lifetimes_and_generics = &generics.params;
    let where_clause = &generics.where_clause;
    let sum = match &data.fields {
        Fields::Named(ref fields) => fields
            .named
            .iter()
            .map(|field| {
                let id = field.ident.as_ref().unwrap();
                quote! { MemoryUsage::size_of(&self.#id) }
            })
            .collect(),
        Fields::Unit => vec![],
        Fields::Unnamed(fields) => (0..(fields.unnamed.iter().count()))
            .into_iter()
            .map(|field| {
                let id = syn::Index::from(field);
                quote! { MemoryUsage::size_of(&self.#id) }
            })
            .collect(),
    }
    .iter()
    // TODO: use Iterator::fold_first once it's stable. https://github.com/rust-lang/rust/pull/79805
    .fold(quote! { 0 }, |x, y| quote! { #x + #y });

    (quote! {
        #[allow(dead_code)]
        impl < #lifetimes_and_generics > MemoryUsage for #struct_name < #lifetimes_and_generics > #where_clause {
            fn size_of(&self) -> usize {
                #sum
            }
        }
    })
    .into()
}
