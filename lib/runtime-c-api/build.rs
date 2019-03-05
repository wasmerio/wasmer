#[cfg(feature = "generate-c-api-headers")]
extern crate cbindgen;

use std::{env, path::Path};

static CAPI_ENV_VAR: &str = "WASM_EMSCRIPTEN_GENERATE_C_API_HEADERS";

fn main() {
    if env::var(CAPI_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        build();
    }
}

#[cfg(feature = "generate-c-api-headers")]
fn build() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);
    let mut wasmer_h = out_path.to_path_buf();
    wasmer_h.push("wasmer.h");
    let mut wasmer_hh = out_path.to_path_buf();
    wasmer_hh.push("wasmer.hh");

    use cbindgen::Language;
    cbindgen::Builder::new()
        .with_crate(crate_dir.clone())
        .with_language(Language::C)
        .with_include_guard("WASMER_H")
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(wasmer_h);

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(Language::Cxx)
        .with_include_guard("WASMER_H")
        .generate()
        .expect("Unable to generate C++ bindings")
        .write_to_file(wasmer_hh);
}

#[cfg(not(feature = "generate-c-api-headers"))]
fn build() {
    panic!("environment var set to generate wasmer c API headers but generate-c-api-headers feature not enabled")
}
