fn main() {
    println!("cargo:rerun-if-changed=src/helpers.c");
    cc::Build::new()
        .warnings(true)
        .file("src/helpers.c")
        .compile("helpers");
}
