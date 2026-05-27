//#AbstractConfigFile: wasi-fyi.config
//#ExpectedStderrFile: io_stderr-beowulf.stderr
use std::io;
use std::io::Write;

fn main() {
  assert!(io::stderr().write_all(include_bytes!("io_stderr-beowulf.stderr")).is_ok());
}
