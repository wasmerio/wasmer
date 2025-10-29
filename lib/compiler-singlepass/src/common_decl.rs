use std::{fs::create_dir_all, io::Write, path::PathBuf};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
}

/// Save assembly output to a given file for debugging purposes
///
/// The output can be disassembled with e.g.:
/// riscv64-linux-gnu-objdump --disassembler-color=on -b binary -m riscv:rv64 -D /path/to/object
#[allow(dead_code)]
pub(crate) fn save_assembly_to_file(suffix: &str, body: &[u8]) {
    let Ok(dir) = std::env::var("SAVE_DIR") else {
        return;
    };

    let base = PathBuf::from(dir);
    create_dir_all(&base).unwrap_or_else(|_| panic!("cannot create dirs: {base:?}"));

    let mut file = tempfile::Builder::new()
        .suffix(suffix)
        .prefix("obj-")
        .tempfile_in(base)
        .expect("Tempfile creation failed");
    file.write_all(body).expect("Write failed");
    let filename = file.keep().expect("persist failed").1;

    eprintln!("Saving assembly output: {filename:?}");
}
