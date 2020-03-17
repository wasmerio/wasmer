use std::env;

mod set_up_toolchain;
mod util;
mod wasi_version;
mod wasitests;

static WASITESTS_ENV_VAR: &str = "WASM_WASI_GENERATE_WASITESTS";
static WASITESTS_SET_UP_TOOLCHAIN: &str = "WASM_WASI_SET_UP_TOOLCHAIN";

fn main() {
    let do_all_wasi_tests = util::should_operate_on_all_wasi_tests();
    if env::var(WASITESTS_SET_UP_TOOLCHAIN).unwrap_or("0".to_string()) == "1" {
        set_up_toolchain::set_it_up(do_all_wasi_tests);
    }

    if env::var(WASITESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        wasitests::build(do_all_wasi_tests);
    }
}
