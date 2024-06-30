use proc_macro2::{Ident, Span, TokenStream};
use syn::{
    parse::{Parse, ParseStream},
    Attribute, Signature, Visibility,
};

#[proc_macro_attribute]
pub fn universal_test(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let MaybeItemFn {
        outer_attrs,
        inner_attrs,
        vis,
        sig,
        block,
    } = syn::parse_macro_input!(item as MaybeItemFn);

    let fn_call = Ident::new(&sig.ident.to_string(), Span::call_site());
    let fn_js_call = Ident::new(&format!("{}_js", sig.ident), Span::call_site());

    let tokens = quote::quote! {
        #[cfg(feature = "js")]
        #[cfg_attr(feature = "js", wasm_bindgen_test)]
        fn #fn_js_call() { #fn_call().unwrap(); }

        #[cfg_attr(any(feature = "sys", feature = "jsc", feature = "wasm-c-api"), test)]
        #(#outer_attrs) *
        #vis #sig
        {
            #(#inner_attrs) *
            #block
        }
    };

    proc_macro::TokenStream::from(tokens)
}

// ---- Taken from tracing::instrument ----
// This is a more flexible/imprecise `ItemFn` type,
// which's block is just a `TokenStream` (it may contain invalid code).
#[derive(Debug, Clone)]
struct MaybeItemFn {
    outer_attrs: Vec<Attribute>,
    inner_attrs: Vec<Attribute>,
    vis: Visibility,
    sig: Signature,
    block: TokenStream,
}

/// This parses a `TokenStream` into a `MaybeItemFn`
/// (just like `ItemFn`, but skips parsing the body).
impl Parse for MaybeItemFn {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let outer_attrs = input.call(Attribute::parse_outer)?;
        let vis: Visibility = input.parse()?;
        let sig: Signature = input.parse()?;
        let inner_attrs = input.call(Attribute::parse_inner)?;
        let block: TokenStream = input.parse()?;
        Ok(Self {
            outer_attrs,
            inner_attrs,
            vis,
            sig,
            block,
        })
    }
}
