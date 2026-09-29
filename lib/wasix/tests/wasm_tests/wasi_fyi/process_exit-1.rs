//#AbstractConfigFile: wasi-fyi.config
//#ExpectedExitCode: 1
use std::process;

fn main() {
    process::exit(1);
}
