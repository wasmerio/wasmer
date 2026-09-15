fn main() {
    println!("cargo:rerun-if-changed=src/engine/unwind/libunwind.c");

    if std::env::var_os("CARGO_CFG_UNIX").is_some() {
        cc::Build::new()
            .file("src/engine/unwind/libunwind.c")
            .warnings(true)
            .compile("wasmer_unwind_detection");
    }
}
