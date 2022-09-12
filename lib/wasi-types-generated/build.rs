use convert_case::{Case, Casing};
use wit_parser::TypeDefKind;

const WIT_1: &str = include_str!("./wit-clean/output.wit");
const BINDINGS_RS: &str = include_str!("./src/wasi/bindings.rs");

fn main() {
    /*
    let bindings_rs = BINDINGS_RS
        .replace("#[allow(clippy::all)]", "")
        .replace("pub mod output {", "")
        .replace("mod output {", "")
        .replace("    ", "");

    let mut bindings_rs = bindings_rs.lines().collect::<Vec<_>>();
    bindings_rs.pop();
    let bindings_rs = bindings_rs.join("\n");

    let target_path = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = std::path::Path::new(&target_path)
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

        match &i.kind {
            wit_parser::TypeDefKind::Enum(e) => {
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
            _ => {}
        }
    }
    std::fs::write(path, contents).unwrap();
    */
}
