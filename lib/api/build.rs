use cmake::Config;
use std::{
    env,
    path::PathBuf,
    process::Command,
};

fn maybe_clone_repo(dest: PathBuf, repo: &str) {
    if !dest.exists() {
        assert!(Command::new("git")
            .arg("clone")
            .arg("--depth=1")
            .arg(repo)
            .arg(dest)
            .status()
            .unwrap()
            .success());
    }
}

fn main() {
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let wamr_dir = PathBuf::from(crate_root).join("third_party").join("wamr");
    let dist = wamr_dir.join("wasm-micro-runtime");

    maybe_clone_repo(
        dist,
        "https://github.com/bytecodealliance/wasm-micro-runtime.git",
    );
    // if dist_githead.exists() {
    //     println!("cargo:rerun-if-changed={}", dist_githead.display());
    // }

    let dst = Config::new(wamr_dir)
        .always_configure(true)
        .generator("Unix Makefiles")
        .define("CMAKE_BUILD_TYPE", "Release")
        .no_build_target(true)
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
        .header("wasm.h")
        // This is needed if use `#include <nng.h>` instead of `#include "path/nng.h"`
        //.clang_arg("-Inng/src/")
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings");
}
