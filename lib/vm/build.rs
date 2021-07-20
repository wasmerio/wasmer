//! 1. Pass `--export-dynamic` to the linker.
//! 2. Runtime build script compiles C code using setjmp for trap handling.

use std::env;

fn main() {
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-cdylib-link-arg=--export-dynamic");

    println!("cargo:rerun-if-changed=src/trap/handlers.c");

    cc::Build::new()
        .warnings(true)
        .define(
            &format!(
                "CFG_TARGET_OS_{}",
                env::var("CARGO_CFG_TARGET_OS").unwrap().to_uppercase()
            ),
            None,
        )
        .file("src/trap/handlers.c")
        .compile("handlers");
}
