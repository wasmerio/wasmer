use std::env;

mod spectests;

static SPECTESTS_ENV_VAR: &str = "WASMER_RUNTIME_GENERATE_SPECTESTS";

fn main() {
    if env::var(SPECTESTS_ENV_VAR).unwrap_or("0".to_string()) == "1" {
        spectests::build();
    }
}
