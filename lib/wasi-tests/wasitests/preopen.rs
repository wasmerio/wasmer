// Args:
// mapdir: /act1:wasitests/test_fs/hamlet/act1
// mapdir: ./act2:wasitests/test_fs/hamlet/act2
// mapdir: act3:wasitests/test_fs/hamlet/act3
// dir: .

#[cfg(not(target_os = "wasi"))]
fn main() {
    let correct_results = [
        ("act1", true) , ("/act1", true) , ("./act1", false),
        ("act2", true) , ("/act2", false), ("./act2", true) ,
        ("act3", true) , ("/act3", false), ("./act3", true) ,
    ];

    for (path, exists) in &correct_results {
        println!("Path `{}` exists? {}", path, exists);
    }
}

#[cfg(target_os = "wasi")]
fn main() {
    use std::path::Path;
    
    let paths = ["act1", "act2", "act3"];
    let prefixes = ["", "/", "./"];

    for path in &paths {
        for prefix in &prefixes {
            let path_name = format!("{}{}", prefix, path);
            let path = Path::new(&path_name);
            println!("Path `{}` exists? {}", &path_name, path.exists());
        }
    }
}
