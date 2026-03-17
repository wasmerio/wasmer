fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/v8_shims.cc");

    cc::Build::new()
        .cpp(true)
        .file("src/v8_shims.cc")
        .flag_if_supported("-std=c++17")
        .compile("snapi_v8_shims");
}
