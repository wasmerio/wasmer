use std::env;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    cbindgen::Builder::new()
        .with_config(cbindgen::Config {
            sort_by: cbindgen::SortKey::Name,
            cpp_compat: true,
            ..cbindgen::Config::default()
        })
        .with_language(cbindgen::Language::C)
        .with_crate(crate_dir)
        .with_include_guard("WASMER_WASI_EXPERIMENTAL_NETWORK")
        .with_documentation(true)
        .generate()
        .expect("Failed to generate C bindings")
        .write_to_file("wasmer_wasi_experimental_network.h");
}
