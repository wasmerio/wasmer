#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

pub mod types;
pub mod wasi;

// Prevent the CI from passing if the wasi/bindings.rs is not
// up to date with the output.wit file
#[test]
#[cfg(feature = "sys")]
fn fail_if_wit_files_arent_up_to_date() {
    use wit_bindgen_core::Generator;

    let output_wit = concat!(env!("CARGO_MANIFEST_DIR"), "/wit-clean/output.wit");
    let bindings_target =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/wasi/bindings.rs"));
    let mut generator = wit_bindgen_rust_wasm::Opts {
        ..wit_bindgen_rust_wasm::Opts::default()
    }
    .build();
    let output_wit_parsed = wit_parser::Interface::parse_file(output_wit).unwrap();
    let imports = vec![output_wit_parsed];
    let exports = vec![];
    let mut files = Default::default();
    generator.generate_all(
        &imports, &exports, &mut files, /* generate_structs */ true,
    );
    let generated = files
        .iter()
        .filter_map(|(k, v)| if k == "bindings.rs" { Some(v) } else { None })
        .next()
        .unwrap();
    let generated_str = String::from_utf8_lossy(generated);
    let generated_str = generated_str
        .lines()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join("\r\n");
    let generated_str = generated_str.replace("mod output {", "pub mod output {");
    let bindings_target = bindings_target
        .lines()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join("\r\n");
    pretty_assertions::assert_eq!(generated_str, bindings_target); // output.wit out of date? regenerate bindings.rs
}
