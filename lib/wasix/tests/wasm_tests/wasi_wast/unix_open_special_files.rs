//#AbstractConfigFile: wasi-wast.config
//#FileSystems: Host
//#MappedDirectory: /dev:/dev
//#ExpectedStdoutFile: unix_open_special_files.stdout

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
