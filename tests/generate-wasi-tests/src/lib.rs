use std::env;

pub mod set_up_toolchain;
pub mod util;
pub mod wasi_version;
pub mod wasitests;

static WASITESTS_ENV_VAR: &str = "WASM_WASI_GENERATE_WASITESTS";
static WASITESTS_SET_UP_TOOLCHAIN: &str = "WASM_WASI_SET_UP_TOOLCHAIN";
static WASITESTS_GENERATE_ALL: &str = "WASI_TEST_GENERATE_ALL";

pub fn build() {
    //println!("cargo:rerun-if-changed=tests/wasi_test_resources/*.rs");
    println!("cargo:rerun-if-env-changed={}", WASITESTS_ENV_VAR);
    println!("cargo:rerun-if-env-changed={}", WASITESTS_SET_UP_TOOLCHAIN);
    println!("cargo:rerun-if-env-changed={}", WASITESTS_GENERATE_ALL);
    let do_all_wasi_tests = util::should_operate_on_all_wasi_tests();
    if env::var(WASITESTS_SET_UP_TOOLCHAIN).unwrap_or("0".to_string()) == "1" {
        set_up_toolchain::set_it_up(do_all_wasi_tests);
    }

    if env::var(WASITESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        wasitests::build(do_all_wasi_tests);
    }
}
