//! This build script sets the default compiler using special output.
//!
//! See https://doc.rust-lang.org/cargo/reference/build-scripts.html
//! for details.

/// This function tells Cargo which default backend to use
fn set_default_backend() {
    // we must use this rather than `cfg` because the build.rs executes on the
    // compiling system.  This method works with cross compilation too.
    match std::env::var("CARGO_CFG_TARGET_ARCH")
        .expect("compilation target env var")
        .as_ref()
    {
        "x86_64" => {
            println!("cargo:rustc-cfg=feature=\"default-backend-cranelift\"");
        }
        "aarch64" => {
            println!("cargo:rustc-cfg=feature=\"default-backend-singlepass\"");
        }
        other => {
            println!("cargo:warning=compiling for untested architecture: \"{}\"!  Attempting to use LLVM", other);
            println!("cargo:rustc-cfg=feature=\"default-backend-llvm\"");
        }
    }
}

/// This function checks if the user specified a default backend
fn has_default_backend() -> bool {
    std::env::var("CARGO_FEATURE_DEFAULT_BACKEND_SINGLEPASS").is_ok()
        || std::env::var("CARGO_FEATURE_DEFAULT_BACKEND_CRANELIFT").is_ok()
        || std::env::var("CARGO_FEATURE_DEFAULT_BACKEND_LLVM").is_ok()
}

fn main() {
    if !has_default_backend() {
        set_default_backend();
    }
}
