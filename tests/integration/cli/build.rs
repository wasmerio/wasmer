fn main() {
    println!(
        "cargo:rustc-env=CARGO_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
