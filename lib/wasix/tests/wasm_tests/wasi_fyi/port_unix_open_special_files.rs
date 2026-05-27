//#AbstractConfigFile: wasi-fyi.config
//#ExpectedStdoutFile: port_unix_open_special_files.stdout
// WASI:
// mapdir: /dev:/dev

use std::fs;
use std::io::Write;

fn main() {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap();
    let result = f.write(b"hello, world!").unwrap();
    println!("{}", result);
}
