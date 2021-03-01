use cc;
use std::process::Command;

fn main() {
    Command::new("make").status().unwrap();
    cc::Build::new()
        .file("src/probes_ffi.c")
        .compile("probes_ffi");
}
