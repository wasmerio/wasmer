use std::io;
use std::io::Write;

fn main() {
  assert!(io::stdout().write_all(include_bytes!("io_stdout-beowulf.stdout")).is_ok());
}
