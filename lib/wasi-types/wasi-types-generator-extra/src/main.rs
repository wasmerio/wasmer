//! This generator is run when regenerate.sh is executed and fixes a couple
//! of issues that wit-bindgen currently doesn't support.
//!
//! Eventually this functionality should be upstreamed into wit-bindgen,
//! see issue [#3177](https://github.com/wasmerio/wasmer/issues/3177).

use convert_case::{Case, Casing};
use quote::quote;
use wit_parser::TypeDefKind;

const WIT_1: &str = include_str!("../../wit-clean/output.wit");
const BINDINGS_RS: &str = include_str!("../../src/wasi/bindings.rs");

fn replace_in_string(s: &str, id: &str, ty: &str) -> String {
    let parts = s.split(&format!("impl {id} {{")).collect::<Vec<_>>();
    if parts.len() == 1 {
        return s.to_string();
    }
    let replaced = parts[1].replacen(
        "from_bits_preserve(bits: u8)",
        &format!("from_bits_preserve(bits: {ty})"),
        1,
    );
    format!("{}impl {id} {{ {replaced}", parts[0])
}

fn find_attr_by_name_mut<'a>(
    mut attrs: impl Iterator<Item = &'a mut syn::Attribute>,
    name: &str,
) -> Option<&'a mut syn::Attribute> {
    attrs.find(|attr| {
        if let Some(ident) = attr.path.get_ident() {
            ident.to_string() == name
        } else {
            false
        }
    })
}

struct Types(Vec<syn::Path>);

impl syn::parse::Parse for Types {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let result =
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated(input)?;
        let items = result.into_iter().collect();
        Ok(Self(items))
    }
}

/// Fix up type definitions for bindings.
fn visit_item(item: &mut syn::Item) {
    match item {
        syn::Item::Enum(enum_) => {
            let name = enum_.ident.to_string();
            // Fix integer representation size for enums.
            let repr_attr = find_attr_by_name_mut(enum_.attrs.iter_mut(), "repr");

            // Change enum repr type.
            match name.as_str() {
                "Clockid" | "Snapshot0Clockid" | "BusErrno" => {
                    repr_attr.unwrap().tokens = quote!((u32));
                }
                "Errno" | "Socktype" | "Addressfamily" | "Sockproto" => {
                    repr_attr.unwrap().tokens = quote!((u16));
                }
                _ => {}
            }

            // Add additional derives.

            match name.as_str() {
                "Clockid" => {
                    let attr = find_attr_by_name_mut(enum_.attrs.iter_mut(), "derive").unwrap();
                    let mut types = attr
                        .parse_args::<Types>()
                        .unwrap()
                        .0;

                    let prim = syn::parse_str::<syn::Path>("num_enum::TryFromPrimitive").unwrap();
                    types.push(prim);

                    let prim = syn::parse_str::<syn::Path>("Hash").unwrap();
                    types.push(prim);

                    attr.tokens = quote!( ( #( #types ),* ) );
                }
                "Signal" => {
                    let attr = find_attr_by_name_mut(enum_.attrs.iter_mut(), "derive").unwrap();
                    let mut types = attr
                        .parse_args::<Types>()
                        .unwrap()
                        .0;
                    let prim = syn::parse_str::<syn::Path>("num_enum::TryFromPrimitive").unwrap();
                    types.push(prim);
                    attr.tokens = quote!( ( #( #types ),* ) );
                }
                _ => {}
            }
        }
        // syn::Item::Struct(struct_) => {}
        syn::Item::Mod(module) => {
            if let Some((_delimiter, children)) = &mut module.content {
                children.iter_mut().for_each(visit_item);
            }
        }
        _ => {}
    }
}

fn main() {
    let mut bindings_rs = BINDINGS_RS
        .replace("#[allow(clippy::all)]", "")
        .replace("pub mod output {", "")
        .replace("mod output {", "")
        .replace("pub struct Rights: u8 {", "pub struct Rights: u64 {")
        .replace("pub struct Lookup: u8 {", "pub struct Lookup: u32 {")
        .replace("pub struct Oflags: u8 {", "pub struct Oflags: u16 {")
        .replace(
            "pub struct Subclockflags: u8 {",
            "pub struct Subclockflags: u16 {",
        )
        .replace(
            "pub struct Eventrwflags: u8 {",
            "pub struct Eventrwflags: u16 {",
        )
        .replace("pub struct Fstflags: u8 {", "pub struct Fstflags: u16 {")
        .replace("pub struct Fdflags: u8 {", "pub struct Fdflags: u16 {");

    bindings_rs = replace_in_string(&bindings_rs, "Oflags", "u16");
    bindings_rs = replace_in_string(&bindings_rs, "Subclockflags", "u16");
    bindings_rs = replace_in_string(&bindings_rs, "Eventrwflags", "u16");
    bindings_rs = replace_in_string(&bindings_rs, "Fstflags", "u16");
    bindings_rs = replace_in_string(&bindings_rs, "Fdflags", "u16");
    bindings_rs = replace_in_string(&bindings_rs, "Lookup", "u32");
    bindings_rs = replace_in_string(&bindings_rs, "Rights", "u64");

    let mut bindings_rs = bindings_rs.lines().collect::<Vec<_>>();
    bindings_rs.pop();
    let bindings_rs = bindings_rs.join("\n");

    // Fix enum types.
    let mut bindings_file = syn::parse_str::<syn::File>(&bindings_rs).unwrap();
    bindings_file.items.iter_mut().for_each(visit_item);
    let bindings_rs = quote!(#bindings_file).to_string();

    let target_path = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(&target_path)
        .parent()
        .unwrap()
        .join("src")
        .join("wasi")
        .join("extra.rs");
    let result = wit_parser::Interface::parse("output.wit", WIT_1).unwrap();
    let mut contents = format!(
        "
        use std::mem::MaybeUninit;
        use wasmer::ValueType;
        // TODO: Remove once bindings generate wai_bindgen_rust::bitflags::bitflags!  (temp hack)
        use wai_bindgen_rust as wit_bindgen_rust;

        {bindings_rs}

    "
    )
    .replace("        ", "");

    println!("output to {}", path.display());

    let excluded_from_impl_valuetype = ["Prestat"];

    for (_, i) in result.types.iter() {
        let name = i.name.clone().unwrap_or_default().to_case(Case::Pascal);
        if name.is_empty() {
            eprintln!(
                "WARNING: skipping extra trait generation for type without name: {:?}",
                i
            );
            continue;
        }

        match i.kind {
            TypeDefKind::Tuple(_) => {
                eprintln!("Skipping extra trait generation for tupe type {:?}", i);
                continue;
            }
            | TypeDefKind::Record(_)
            | TypeDefKind::Flags(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Option(_)
            | TypeDefKind::Expected(_)
            | TypeDefKind::Union(_)
            | TypeDefKind::List(_)
            | TypeDefKind::Future(_)
            | TypeDefKind::Stream(_)
            // | TypeDefKind::Type(_)
            => {
                if excluded_from_impl_valuetype.iter().any(|s| *s == name.as_str()) {
                    continue;
                }
                contents.push_str(&format!("
                    // TODO: if necessary, must be implemented in wit-bindgen
                    unsafe impl ValueType for {name} {{
                        #[inline]
                        fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {{ }}
                    }}

                ").replace("                    ", ""))
            },
            _ => { }
        }

        if let wit_parser::TypeDefKind::Enum(e) = &i.kind {
            contents.push_str(
                &format!(
                    "
            unsafe impl wasmer::FromToNativeWasmType for {name} {{
                type Native = i32;

                fn to_native(self) -> Self::Native {{
                    self as i32
                }}

                fn from_native(n: Self::Native) -> Self {{
                    match n {{\n"
                )
                .replace("                ", ""),
            );

            for (i, case) in e.cases.iter().enumerate() {
                contents.push_str(&format!(
                    "            {i} => Self::{},\n",
                    case.name.to_case(Case::Pascal)
                ));
            }
            contents.push_str(
                &format!(
                    "
                        q => todo!(\"could not serialize number {{q}} to enum {name}\"),
                    }}
                }}

                fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {{ false }}
            }}
            "
                )
                .replace("                ", ""),
            );
        }
    }
    std::fs::write(path, contents).unwrap();
}
