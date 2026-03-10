use std::env;

mod emtests;

static EMTESTS_ENV_VAR: &str = "WASM_EMSCRIPTEN_GENERATE_EMTESTS";

fn main() {
    if env::var(EMTESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        emtests::build();
    }
}
