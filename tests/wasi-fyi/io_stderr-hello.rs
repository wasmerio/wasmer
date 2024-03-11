use std::io;
use std::io::Write;

fn main() {
  assert!(io::stderr().write_all(include_bytes!("io_stderr-hello.stderr")).is_ok());
}
