fn main() {
    println!(
        "cargo:rustc-env=CFG_TARGET_OS={}",
        std::env::var("CARGO_CFG_TARGET_OS")
            .expect("CARGO_CFG_TARGET_OS must be provided by cargo")
    );
    println!(
        "cargo:rustc-env=CFG_TARGET_ARCH={}",
        std::env::var("CARGO_CFG_TARGET_ARCH")
            .expect("CARGO_CFG_TARGET_ARCH must be provided by cargo")
    );
    println!(
        "cargo:rustc-env=CFG_TARGET_ENV={}",
        std::env::var("CARGO_CFG_TARGET_ENV")
            .expect("CARGO_CFG_TARGET_ENV must be provided by cargo")
    );
}
