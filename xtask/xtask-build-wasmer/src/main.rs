use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn package_wasmer(out: &PathBuf) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    let tar = File::create(out).unwrap();
    let enc = GzEncoder::new(tar, Compression::none());
    let mut a = tar::Builder::new(enc);

    let in_dir = project_root();
    let bin_dir = project_root().join("package").join("bin");
    let release_dir = in_dir.join("target").join("release");

    std::fs::create_dir_all(&bin_dir).unwrap();

    let libwasmer_path = release_dir.join("wasmer");
    if libwasmer_path.exists() {
        std::fs::copy(&libwasmer_path, bin_dir.join("wasmer")).unwrap();
    }

    let libwasmer_path = release_dir.join("wasmer.exe");
    if libwasmer_path.exists() {
        std::fs::copy(&libwasmer_path, bin_dir.join("wasmer.exe")).unwrap();
    }

    a.append_dir_all("bin", &bin_dir).unwrap();
    a.finish().unwrap();
}

fn main() {
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .collect::<Vec<_>>()
        .join(",");
    if !compiler_features.is_empty() {
        compiler_features.push(',');
    };
    let mut cmd = Command::new(cargo);
    cmd.current_dir(project_root());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.args(&[
        "build", 
        "--release",
        "--manifest-path",
        "lib/cli/Cargo.toml",
        "--features",
        "webc_runner",
        "--features",
        &format!("{compiler_features}wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
        "--bin",
        "wasmer",
    ]);

    println!("running {cmd:?}");

    let cmd = cmd.output().unwrap();

    if !cmd.status.success() {
        println!("build capi paniced with status {}", cmd.status);
        println!("stdout: {}", String::from_utf8_lossy(&cmd.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&cmd.stdout));
        panic!("build capi failed");
    }

    println!("build wasmer succeeded, compilers = {compilers}");

    let out_path = project_root().join("build-wasmer.tar.gz");
    package_wasmer(&out_path);

    println!("packaged to = {}", out_path.display());
}
