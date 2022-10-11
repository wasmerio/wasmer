//! This generator is run when regenerate.sh is executed and fixes a couple
//! of issues that wit-bindgen currently doesn't support.
//!
//! Eventually this functionality should be upstreamed into wit-bindgen,
//! see issue [#3177](https://github.com/wasmerio/wasmer/issues/3177).

use convert_case::{Case, Casing};
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

        {bindings_rs}

    "
    )
    .replace("        ", "");

    println!("output to {}", path.display());

    let excluded_from_impl_valuetype = ["Prestat"];

    for (_, i) in result.types.iter() {
        match i.kind {
            | TypeDefKind::Record(_)
            | TypeDefKind::Flags(_)
            | TypeDefKind::Tuple(_)
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
                let name = i.name.clone().unwrap_or_default().to_case(Case::Pascal);
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

        let name = i.name.clone().unwrap_or_default().to_case(Case::Pascal);

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
