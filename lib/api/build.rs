fn main() {
    #[cfg(feature = "wamr")]
    {
        use cmake::Config;
        use std::{env, path::PathBuf};

        let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
        let wamr_dir = PathBuf::from(&crate_root).join("third_party").join("wamr");

        let mut fetch_submodules = std::process::Command::new("git");
        fetch_submodules
            .current_dir(crate_root)
            .arg("submodule")
            .arg("update")
            .arg("--init");

        let res = fetch_submodules.output();

        if let Err(e) = res {
            panic!("fetching submodules failed: {e}");
        }

        let dst = Config::new(wamr_dir.clone())
            .always_configure(true)
            .generator("Unix Makefiles")
            .define(
                "CMAKE_BUILD_TYPE",
                if cfg!(debug_assertions) {
                    "RelWithDebInfo"
                } else {
                    "Release"
                },
            )
            .define("WAMR_BUILD_AOT", "0")
            //.define("WAMR_BUILD_TAIL_CALL", "1")
            //.define("WAMR_BUILD_DUMP_CALL_STACK", "1")
            // .define("WAMR_BUILD_CUSTOM_NAME_SECTION", "1")
            // .define("WAMR_BUILD_LOAD_CUSTOM_SECTION", "1")
            .define("WAMR_BUILD_BULK_MEMORY", "1")
            .define("WAMR_BUILD_REF_TYPES", "1")
            .define("WAMR_BUILD_SIMD", "1")
            .define("WAMR_ENABLE_FAST_INTERP", "1")
            .define("WAMR_BUILD_LIB_PTHREAD", "1")
            .define("WAMR_BUILD_LIB_WASI_THREADS", "0")
            .define("WAMR_BUILD_LIBC_WASI", "0")
            .define("WAMR_BUILD_LIBC_BUILTIN", "0")
            .define("WAMR_BUILD_SHARED_MEMORY", "1")
            .define("WAMR_BUILD_MULTI_MODULE", "0")
            .define("WAMR_DISABLE_HW_BOUND_CHECK", "1")
            .build();

        // Check output of `cargo build --verbose`, should see something like:
        // -L native=/path/runng/target/debug/build/runng-sys-abc1234/out
        // That contains output from cmake
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("build").display()
        );
        println!("cargo:rustc-link-lib=vmlib");

        let bindings = bindgen::Builder::default()
            .header(
                wamr_dir
                    .join("core/iwasm/include/wasm_c_api.h")
                    .to_str()
                    .unwrap(),
            )
            .header(
                wamr_dir
                    .join("core/iwasm/include/wasm_c_api.h")
                    .to_str()
                    .unwrap(),
            )
            .header(
                wamr_dir
                    .join("core/iwasm/include/wasm_export.h")
                    .to_str()
                    .unwrap(),
            )
            .derive_default(true)
            .derive_debug(true)
            .generate()
            .expect("Unable to generate bindings");
        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("bindings.rs"))
            .expect("Couldn't write bindings");
    }
}
