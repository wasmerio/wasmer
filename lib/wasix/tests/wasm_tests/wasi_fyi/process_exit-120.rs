//#AbstractConfigFile: wasi-fyi.config
//#ExpectedExitCode: 120
use std::process;

fn main() {
    process::exit(120);
}
