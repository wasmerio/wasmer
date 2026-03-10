use std::env;

mod wasitests;

static WASITESTS_ENV_VAR: &str = "WASM_WASI_GENERATE_WASITESTS";

fn main() {
    if env::var(WASITESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        wasitests::build();
    }
}
