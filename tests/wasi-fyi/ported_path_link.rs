// WASI:
// mapdir: act5:test_fs/hamlet/act5
// tempdir: temp

use std::fs;
use std::io::Read;

fn main() {
    {
        std::fs::hard_link("/hamlet/act5/scene1.txt", "/tmp/scene_of_the_day.txt").unwrap();
        let mut f = fs::OpenOptions::new()
            .read(true)
            .open("/tmp/scene_of_the_day.txt")
            .unwrap();
        let mut buffer = [0u8; 64];
        f.read_exact(&mut buffer).unwrap();

        println!("{}", std::str::from_utf8(&buffer[..]).unwrap());
        for b in buffer.iter_mut() {
            *b = 0;
        }

        let mut f = fs::OpenOptions::new()
            .read(true)
            .open("/hamlet/act5/scene1.txt")
            .unwrap();
        f.read_exact(&mut buffer).unwrap();
        println!("{}", std::str::from_utf8(&buffer[..]).unwrap());
    }

    std::fs::remove_file("/tmp/scene_of_the_day.txt").unwrap();
    let path = std::path::PathBuf::from("act5/scene1.txt");

    if path.exists() {
        println!("Path still exists");
    } else {
        println!("Path was deleted!");
    }
}
