use std::io::{Read, Write};

fn main() {
    let mut stdin = ::std::io::stdin();
    let mut stdout = ::std::io::stdout();
    let mut buf: Vec<u8> = vec![0; 512];
    let mut total: u64 = 0;
    while total < 1048576u64 * 2048 {
        let n = stdin.read(&mut buf).unwrap();
        stdout.write_all(&buf[..n]).unwrap();
        total += n as u64;
    }
}
