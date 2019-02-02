#[cfg(feature = "generate-c-api-headers")]
extern crate cbindgen;

use std::env;

static CAPI_ENV_VAR: &str = "WASM_EMSCRIPTEN_GENERATE_C_API_HEADERS";

fn main() {
    if env::var(CAPI_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        build();
    }
}

#[cfg(feature = "generate-c-api-headers")]
fn build() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    use cbindgen::Language;
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(Language::C)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("wasmer.h");
}

#[cfg(not(feature = "generate-c-api-headers"))]
fn build() {
    panic!("environment var set to generate wasmer c API headers but generate-c-api-headers feature not enabled")
}
