//! Pass `--export-dynamic` to the linker.
fn main() {
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-cdylib-link-arg=--export-dynamic");
}
