fn main() {
    println!("cargo:rerun-if-changed=../../ignores.txt");
    if let Ok(os) = std::env::var("CARGO_CFG_TARGET_OS") {
        println!("cargo:rustc-env=CFG_TARGET_OS={os}");
    }
    if let Ok(os) = std::env::var("CARGO_CFG_TARGET_ARCH") {
        println!("cargo:rustc-env=CFG_TARGET_ARCH={os}");
    }
    if let Ok(os) = std::env::var("CARGO_CFG_TARGET_ENV") {
        println!("cargo:rustc-env=CFG_TARGET_ENV={os}");
    }
}
