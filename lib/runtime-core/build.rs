use blake2b_simd::blake2bp;
use std::{fs, io::Write, path::PathBuf};

const WASMER_VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut state = blake2bp::State::new();
    state.update(WASMER_VERSION.as_bytes());

    let hasher = state.finalize();
    let hash_string = hasher.to_hex().as_str().to_owned();

    let crate_dir = env!("CARGO_MANIFEST_DIR");
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
