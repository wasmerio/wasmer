use std::io;
use std::io::Read;

fn main() {
    let mut stdin = String::new();
    assert!(io::stdin().read_to_string(&mut stdin).is_ok());
    assert_eq!(
        stdin,
        String::from_utf8_lossy(include_bytes!("io_stdin-beowulf.stdin"))
    );
}
