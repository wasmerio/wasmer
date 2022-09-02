extern crate proc_macro;
use proc_macro::TokenStream;
use syn::{parse_macro_input, Result};
use syn::parse::{Parse, ParseStream};
use proc_macro2::{Ident, Span};

struct MyMacroInput {
    function_name: String
}

impl Parse for MyMacroInput {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(MyMacroInput { function_name: format!("hello") })
    }
}

#[proc_macro_attribute]
pub fn universal_test(_: TokenStream, item: TokenStream) -> TokenStream {

    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as MyMacroInput);
    let function_name_normal = Ident::new(&input.function_name, Span::call_site());
    let function_name_js = Ident::new(&format!("{}_js", input.function_name), Span::call_site());
    let item = proc_macro2::TokenStream::from(item);

    let tokens = proc_quote::quote! {
        #[cfg(feature = "js")]
        #[cfg_attr(feature = "js", wasm_bindgen_test)]
        fn #function_name_js() {
            let e: Result<()> = #function_name_normal();
            e.unwrap();
        }
    
        #[cfg_attr(feature = "sys", test)]
        #item
    };

    proc_macro::TokenStream::from(tokens)
}