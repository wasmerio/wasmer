//! Runtime build script compiles C code using setjmp for trap handling.

fn main() {
    println!("cargo:rerun-if-changed=src/trap/helpers.c");
    cc::Build::new()
        .warnings(true)
        .file("src/trap/helpers.c")
        .compile("helpers");
}
