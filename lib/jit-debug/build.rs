fn main() {
    let mut build = cc::Build::new();
    build.warnings(true);
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    build.define(&format!("CFG_TARGET_OS_{os}"), None);

    println!("cargo:rerun-if-changed=gdbjit.c");
    build.file("gdbjit.c");
    build.compile("gdbjit-helpers");
}
