fn main() {
    #[cfg(feature = "wasm-c-api")]
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
            .define("CMAKE_BUILD_TYPE", "Release")
            .define("WAMR_BUILD_AOT", "0")
            .define("WAMR_BUILD_LOAD_CUSTOM_SECTION", "1")
            .define("WAMR_BUILD_CUSTOM_NAME_SECTION", "1")
            //.define("WAMR_ENABLE_FAST_INTERP", "1")
            .define("WAMR_BUILD_SHARED_MEMORY", "1")
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
                    .join("core")
                    .join("iwasm")
                    .join("include")
                    .join("wasm_c_api.h")
                    .to_str()
                    .unwrap(),
            )
            // This is needed if use `#include <nng.h>` instead of `#include "path/nng.h"`
            //.clang_arg("-Inng/src/")
            .generate()
            .expect("Unable to generate bindings");
        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("bindings.rs"))
            .expect("Couldn't write bindings");
    }
}
