extern crate proc_macro;

use proc_macro_error2::proc_macro_error;
use syn::{DeriveInput, parse_macro_input};

mod value_type;

#[proc_macro_error]
#[proc_macro_derive(ValueType)]
pub fn derive_value_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let r#gen = value_type::impl_value_type(&input);
    r#gen.into()
}
