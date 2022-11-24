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

fn package_capi(out: &PathBuf) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    let tar = File::create(out).unwrap();
    let enc = GzEncoder::new(tar, Compression::none());
    let mut a = tar::Builder::new(enc);

    let in_dir = project_root();
    let lib_dir = project_root().join("package").join("lib");
    let include_dir = project_root().join("package").join("include");
    let release_dir = match std::env::var("CARGO_TARGET") {
        Ok(o) => in_dir.join("target").join(o).join("release"),
        Err(_) => in_dir.join("target").join("release"),
    };

    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::create_dir_all(&include_dir).unwrap();

    std::fs::copy(
        project_root().join("lib").join("c-api").join("wasmer.h"),
        include_dir.join("wasmer.h"),
    )
    .unwrap();
    std::fs::copy(
        project_root()
            .join("lib")
            .join("c-api")
            .join("wasmer_wasm.h"),
        include_dir.join("wasmer_wasm.h"),
    )
    .unwrap();
    std::fs::copy(
        project_root()
            .join("lib")
            .join("c-api")
            .join("tests")
            .join("wasm-c-api")
            .join("include")
            .join("wasm.h"),
        include_dir.join("wasm.h"),
    )
    .unwrap();
    std::fs::copy(
        project_root().join("lib").join("c-api").join("README.md"),
        include_dir.join("README.md"),
    )
    .unwrap();

    let _ = std::fs::copy(release_dir.join("wasmer.dll"), lib_dir.join("wasmer.dll"));
    let _ = std::fs::copy(
        release_dir.join("wasmer.dll.lib"),
        lib_dir.join("wasmer.dll.lib"),
    );
    let _ = std::fs::copy(release_dir.join("wasmer.lib"), lib_dir.join("wasmer.lib"));
    let _ = std::fs::copy(
        release_dir.join("libwasmer.dylib"),
        lib_dir.join("libwasmer.dylib"),
    );
    let _ = std::fs::copy(
        release_dir.join("libwasmer.so"),
        lib_dir.join("libwasmer.so"),
    );
    let _ = std::fs::copy(release_dir.join("libwasmer.a"), lib_dir.join("libwasmer.a"));

    a.append_dir_all("lib", &lib_dir).unwrap();
    a.append_dir_all("include", &include_dir).unwrap();
    a.finish().unwrap();
}

fn main() {
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut capi_compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .filter(|i| *i != "llvm")
        .collect::<Vec<_>>()
        .join(",");
    if !capi_compiler_features.is_empty() {
        capi_compiler_features.push(',');
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
        "lib/c-api/Cargo.toml",
        "--no-default-features",
        "--features",
        "wat,compiler,wasi,middlewares,webc_runner",
        "--features",
        &format!("{capi_compiler_features}wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
    ]);

    if let Ok(target) = std::env::var("CARGO_TARGET") {
        cmd.args(&["--target", &target]);
    }

    println!("running {cmd:?}");

    let cmd = cmd.output().unwrap();

    if !cmd.status.success() {
        println!("build capi paniced with status {}", cmd.status);
        println!("stdout: {}", String::from_utf8_lossy(&cmd.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&cmd.stdout));
        panic!("build capi failed");
    }

    println!("build capi succeeded, compilers = {compilers}");

    let out_path = project_root().join("build-capi.tar.gz");
    package_capi(&out_path);
    println!("packaged to {}", out_path.display());
}
