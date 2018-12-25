extern crate wabt;

use std::env;

mod emtests;
mod spectests;

static SPECTESTS_ENV_VAR: &str = "WASM_GENERATE_SPECTESTS";
static EMTESTS_ENV_VAR: &str = "WASM_GENERATE_EMTESTS";

fn main() {
    if env::var(SPECTESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        spectests::build();
    }
    if env::var(EMTESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        emtests::build();
    }
}
