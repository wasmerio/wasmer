extern crate cbindgen;

use cbindgen::{Builder, Language};
use std::{env, fs, path::PathBuf};

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut crate_wasmer_header_file = PathBuf::from(&crate_dir);
    crate_wasmer_header_file.push("wasmer");

    let out_dir = env::var("OUT_DIR").unwrap();
    let mut out_wasmer_header_file = PathBuf::from(&out_dir);
    out_wasmer_header_file.push("wasmer");

    const WASMER_PRE_HEADER: &str = r#"
#if !defined(WASMER_H_MACROS)
#define WASMER_H_MACROS

#if defined(MSVC)
#if defined(_M_AMD64)
#define ARCH_X86_64
#endif
#endif

#if defined(GCC) || defined(__GNUC__) || defined(__clang__)
#if defined(__x86_64__)
#define ARCH_X86_64
#endif
#endif

#endif // WASMER_H_MACROS
"#;
    // Generate the C bindings in the `OUT_DIR`.
    out_wasmer_header_file.set_extension("h");
    Builder::new()
        .with_crate(crate_dir.clone())
        .with_language(Language::C)
        .with_include_guard("WASMER_H")
        .with_header(WASMER_PRE_HEADER)
        .with_define("target_family", "windows", "_WIN32")
        .with_define("target_arch", "x86_64", "ARCH_X86_64")
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(out_wasmer_header_file.as_path());

    // Generate the C++ bindings in the `OUT_DIR`.
    out_wasmer_header_file.set_extension("hh");
    Builder::new()
        .with_crate(crate_dir)
        .with_language(Language::Cxx)
        .with_include_guard("WASMER_H")
        .with_header(WASMER_PRE_HEADER)
        .with_define("target_family", "windows", "_WIN32")
        .with_define("target_arch", "x86_64", "ARCH_X86_64")
        .generate()
        .expect("Unable to generate C++ bindings")
        .write_to_file(out_wasmer_header_file.as_path());

    // Copy the generated C bindings from `OUT_DIR` to
    // `CARGO_MANIFEST_DIR`.
    crate_wasmer_header_file.set_extension("h");
    out_wasmer_header_file.set_extension("h");
    fs::copy(
        out_wasmer_header_file.as_path(),
        crate_wasmer_header_file.as_path(),
    )
    .expect("Unable to copy the generated C bindings");

    // Copy the generated C++ bindings from `OUT_DIR` to
    // `CARGO_MANIFEST_DIR`.
    crate_wasmer_header_file.set_extension("h");
    crate_wasmer_header_file.set_extension("hh");
    out_wasmer_header_file.set_extension("hh");
    fs::copy(out_wasmer_header_file, crate_wasmer_header_file)
        .expect("Unable to copy the generated C++ bindings");
}
