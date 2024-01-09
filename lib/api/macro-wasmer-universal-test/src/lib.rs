extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};

#[proc_macro_attribute]
pub fn universal_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let mut iter = item_clone.into_iter();
    let _ = iter.next().unwrap(); // fn
    let item_tree: proc_macro::TokenTree = iter.next().unwrap(); // fn ...
    let n = match &item_tree {
        proc_macro::TokenTree::Ident(i) => i.to_string(),
        _ => panic!("expected fn ...() -> Result<(), String>"),
    };

    let function_name_normal = Ident::new(&n, Span::call_site());
    let function_name_js = Ident::new(&format!("{}_js", n), Span::call_site());
    let parsed = match syn::parse::<syn::ItemFn>(item) {
        Ok(o) => o,
        Err(e) => {
            return proc_macro::TokenStream::from(e.to_compile_error());
        }
    };

    let tokens = quote::quote! {
        #[cfg(feature = "js")]
        #[cfg_attr(feature = "js", wasm_bindgen_test)]
        fn #function_name_js() { #function_name_normal().unwrap(); }

        #[cfg_attr(any(feature = "sys", feature = "jsc"), test)]
        #parsed
    };

    proc_macro::TokenStream::from(tokens)
}
