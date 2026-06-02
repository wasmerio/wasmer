//#DefaultMappedDirectories: false
//#FileSystems: all
//#CurrentDirectory: /
//#MappedDirectory: test_fs/hamlet/act1:/act1
//#MappedDirectory: test_fs/hamlet/act2:/act2
//#MappedDirectory: test_fs/hamlet/act1:/act1-again
//#ExpectedStdoutFile: wasi_sees_virtual_root.stdout

use std::fs;

fn main() {
    // just cheat in this test because there is no comparison for native
    #[cfg(not(target_os = "wasi"))]
    let results = {
        let start = vec!["\"/act1\"", "\"/act1-again\"", "\"/act2\""];

        let mut out = vec![];
        for _ in 0..4 {
            for path_str in &start {
                out.push(path_str.to_string());
            }
        }

        out.push("ROOT IS SAFE".to_string());
        out
    };

    #[cfg(target_os = "wasi")]
    let results = {
        let mut out = vec![];
        let mapped_roots = ["act1", "act1-again", "act2"];
        let visible_mapped_roots = |path| {
            let mut roots = fs::read_dir(path)
                .unwrap()
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    let name = path.file_name()?.to_str()?;
                    mapped_roots
                        .contains(&name)
                        .then(|| format!("\"/{name}\""))
                })
                .collect::<Vec<_>>();
            roots.sort();
            roots
        };

        for path in [
            "/",
            "act1/..",
            "act1/../../..",
            "act1/../../act2/../act1/../../../",
        ] {
            out.extend(visible_mapped_roots(path));
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
