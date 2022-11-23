use std::env;
use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn main() {
    /*
        exclude_tests := --exclude wasmer-c-api --exclude wasmer-cli --exclude wasmer-compiler-cli
        # Is failing to compile in Linux for some reason
        exclude_tests += --exclude wasmer-wasi-experimental-io-devices
        # We run integration tests separately (it requires building the c-api)
        exclude_tests += --exclude wasmer-integration-tests-cli
        exclude_tests += --exclude wasmer-integration-tests-ios

        ifneq (, $(findstring llvm,$(compilers)))
            ENABLE_LLVM := 1
        else
            # We exclude LLVM from our package testing
            exclude_tests += --exclude wasmer-compiler-llvm
        endif
    */

    // build-capi package-capi
    // $(foreach compiler_engine,$(capi_compilers_engines),test-capi-crate-$(compiler_engine) test-capi-integration-$(compiler_engine))
    /*
        WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-crate-//) $(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
        --no-default-features --features wat,compiler,wasi,middlewares,webc_runner $(capi_compiler_features) -- --nocapture
    */
    // std::fs::read("./build-capi.tar.gz");
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    println!(
        "test capi, compilers = {compilers}, project root = {}",
        project_root().display()
    );
}
