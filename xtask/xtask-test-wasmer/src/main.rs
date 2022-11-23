use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn main() {
    // $(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features)
    let compilers = std::env::var("COMPILERS").unwrap();
    println!("test wasmer, compilers = {compilers}");
}
