extern crate cbindgen;

use cbindgen::{Builder, Language};
use std::{env, path::Path};

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);

    let mut wasmer_h = out_path.to_path_buf();
    wasmer_h.push("wasmer.h");

    let mut wasmer_hh = out_path.to_path_buf();
    wasmer_hh.push("wasmer.hh");

    Builder::new()
        .with_crate(crate_dir.clone())
        .with_language(Language::C)
        .with_include_guard("WASMER_H")
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(wasmer_h);

    Builder::new()
        .with_crate(crate_dir)
        .with_language(Language::Cxx)
        .with_include_guard("WASMER_H")
        .generate()
        .expect("Unable to generate C++ bindings")
        .write_to_file(wasmer_hh);
}
