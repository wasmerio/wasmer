//! Runtime build script compiles C code using setjmp for trap handling.

use std::env;

fn main() {
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
