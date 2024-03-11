// WASI:
// tempdir: .

use std::fs;
use std::path::PathBuf;

fn main() {
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from(".");
    #[cfg(target_os = "wasi")]
    {
        base.push("fd_sync_file.txt");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&base)
            .expect("Could not create file");

        {
            use std::io::Write;
            // example text from https://www.un.org/en/universal-declaration-human-rights/
            file.write_all(b"All human beings are born free and equal in dignity and rights. They are endowed with reason and conscience and should act towards one another in a spirit of brotherhood.").unwrap();
            file.sync_all().unwrap();
            let len = file.metadata().unwrap().len();
            println!("{}", len);
            assert_eq!(len, 170);
            file.set_len(170 + 1234).unwrap();
            file.sync_all().unwrap();
            let len = file.metadata().unwrap().len();
            println!("{}", len);
            assert_eq!(len, 1234 + 170);
        }
    }
    #[cfg(target_os = "wasi")]
    std::fs::remove_file(&base).unwrap();

    #[cfg(not(target_os = "wasi"))]
    {
        // eh, just print the output directly
        println!("170");
        println!("1404");
    }
}
