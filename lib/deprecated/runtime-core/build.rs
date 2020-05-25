use blake3::Hasher;
use std::{env, fs, io::Write, path::PathBuf};

const WASMER_VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    // `wasmer_version_hash.txt`
    {
        let mut hasher = Hasher::new();
        hasher.update(WASMER_VERSION.as_bytes());

        let hasher = hasher.finalize();
        let hash_string = hasher.to_hex().as_str().to_owned();

        let crate_dir = env::var("OUT_DIR").unwrap();
        let wasmer_version_hash_file = {
            let mut path = PathBuf::from(&crate_dir);
            path.push("wasmer_version_hash.txt");
            path
        };

        let mut f_out = fs::File::create(wasmer_version_hash_file)
            .expect("Could not create file for wasmer hash value");

        f_out
            .write_all(hash_string.as_bytes())
            .expect("Could not write to file for wasmer hash value");
    }
}
