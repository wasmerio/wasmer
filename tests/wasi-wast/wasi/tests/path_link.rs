// WASI:
// mapdir: act5:test_fs/hamlet/act5
// tempdir: temp

use std::fs;
use std::io::Read;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    {
        let out_str = "ACT V
SCENE I. A churchyard.

    Enter two Clowns, with spades,";
        println!("{}", out_str);
        println!("{}", out_str);
        println!("Path still exists");
    }

    #[cfg(target_os = "wasi")]
    {
        {
            std::fs::hard_link("act5/scene1.txt", "temp/scene_of_the_day.txt").unwrap();
            let mut f = fs::OpenOptions::new()
                .read(true)
                .open("temp/scene_of_the_day.txt")
                .unwrap();
            let mut buffer = [0u8; 64];
            f.read_exact(&mut buffer).unwrap();

            println!("{}", std::str::from_utf8(&buffer[..]).unwrap());
            for b in buffer.iter_mut() {
                *b = 0;
            }

            let mut f = fs::OpenOptions::new()
                .read(true)
                .open("act5/scene1.txt")
                .unwrap();
            f.read_exact(&mut buffer).unwrap();
            println!("{}", std::str::from_utf8(&buffer[..]).unwrap());
        }

        std::fs::remove_file("temp/scene_of_the_day.txt").unwrap();
        let path = std::path::PathBuf::from("act5/scene1.txt");

        if path.exists() {
            println!("Path still exists");
        } else {
            println!("Path was deleted!");
        }
    }
}
