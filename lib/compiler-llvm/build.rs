fn main() {
    #[cfg(windows)]
    configure_dylib_windows();
}

#[cfg(windows)]
fn configure_dylib_windows() {
    #[cfg(debug_assertions)]
    let lib = "msvcrtd";
    #[cfg(not(debug_assertions))]
    let lib = "msvcrt";
    println!("cargo:rustc-link-lib={}", lib);
}
