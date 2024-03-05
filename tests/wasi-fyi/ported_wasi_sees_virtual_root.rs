// WASI:
// mapdir: act1:test_fs/hamlet/act1
// mapdir: act2:test_fs/hamlet/act2
// mapdir: act1-again:test_fs/hamlet/act1

use std::fs;

fn main() {
    let results = {
        let mut out = vec![];

        let read_dir = fs::read_dir("/").unwrap();
        for entry in read_dir {
            out.push(format!("{:?}", entry.unwrap().path()))
        }
        let read_dir = fs::read_dir("/hamlet/act1/..").unwrap();
        for entry in read_dir {
            out.push(format!("{:?}", entry.unwrap().path()))
        }
        let read_dir = fs::read_dir("/hamlet/act1/../../..").unwrap();
        for entry in read_dir {
            out.push(format!("{:?}", entry.unwrap().path()))
        }
        let read_dir = fs::read_dir("/hamlet/act1/../../hamlet/act2/../act1/../../../").unwrap();
        for entry in read_dir {
            out.push(format!("{:?}", entry.unwrap().path()))
        }
        let f = fs::OpenOptions::new().write(true).open("/abc");

        if f.is_ok() {
            out.push("ROOT IS NOT SAFE".to_string());
        } else {
            out.push("ROOT IS SAFE".to_string());
        }

        out
    };

    for result in results {
        println!("{}", result);
    }
}
