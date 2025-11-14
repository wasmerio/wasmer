use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Member};

/// We can only validate types that have a well defined layout.
fn check_repr(input: &DeriveInput) {
    if input.attrs.iter().any(|attr| {
        attr.path().is_ident("repr") && {
            let mut valid = false;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("C") || meta.path.is_ident("transparent") {
                    valid = true;
                }
                Ok(())
            });
            valid
        }
    }) {
        return;
    }

    abort!(
        input,
        "ValueType can only be derived for #[repr(C)] or #[repr(transparent)] structs"
    )
}

/// Zero out any padding bytes between fields.
fn zero_padding(fields: &Fields) -> TokenStream {
    let names: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| match &field.ident {
            Some(ident) => Member::Named(ident.clone()),
            None => Member::Unnamed(i.into()),
        })
        .collect();

    let mut out = TokenStream::new();
    for i in 0..fields.len() {
        let name = &names[i];
        let start = quote! {
            &self.#name as *const _ as usize - self as *const _ as usize
        };
        let len = quote! {
            ::core::mem::size_of_val(&self.#name)
        };
        let end = quote! {
            #start + #len
        };

        // Zero out padding bytes within the current field.
        //
        // This also ensures that all fields implement ValueType.
        out.extend(quote! {
            ::wasmer_types::ValueType::zero_padding_bytes(&self.#name, &mut _bytes[#start..(#start + #len)]);
        });

        let padding_end = if i == fields.len() - 1 {
            // Zero out padding bytes between the last field and the end of the struct.
            let total_size = quote! {
                ::core::mem::size_of_val(self)
            };
            total_size
        } else {
            // Zero out padding bytes between the current field and the next one.
            let next_name = &names[i + 1];
            let next_start = quote! {
                &self.#next_name as *const _ as usize - self as *const _ as usize
            };
            next_start
        };
        out.extend(quote! {
            for i in #end..#padding_end {
                _bytes[i] = ::core::mem::MaybeUninit::new(0);
            }
        });
    }
    out
}

pub fn impl_value_type(input: &DeriveInput) -> TokenStream {
    check_repr(input);

    let struct_name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let fields = match &input.data {
        Data::Struct(ds) => &ds.fields,
        _ => abort!(input, "ValueType can only be derived for structs"),
    };

    let zero_padding = zero_padding(fields);

    quote! {
        unsafe impl #impl_generics ::wasmer_types::ValueType for #struct_name #ty_generics #where_clause {
            #[inline]
            fn zero_padding_bytes(&self, _bytes: &mut [::core::mem::MaybeUninit<u8>]) {
                #zero_padding
            }
        }
    }
}
