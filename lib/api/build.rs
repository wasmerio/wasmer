use std::env;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(stub_backend)");
    let real_backend_features = [
        "CARGO_FEATURE_SYS",
        "CARGO_FEATURE_WAMR",
        "CARGO_FEATURE_WASMI",
        "CARGO_FEATURE_V8",
        "CARGO_FEATURE_JS",
        "CARGO_FEATURE_JSC",
    ];

    let has_real_backend = real_backend_features
        .iter()
        .any(|feature| env::var(feature).is_ok());

    let enable_stub = env::var("CARGO_FEATURE_STUB").is_ok() || !has_real_backend;

    if enable_stub {
        println!("cargo:rustc-cfg=stub_backend");
    }
}

