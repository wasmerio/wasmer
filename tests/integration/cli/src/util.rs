use crate::get_repo_root_path;
use anyhow::bail;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Copy, Clone)]
pub enum Compiler {
    Cranelift,
    LLVM,
    Singlepass,
}

impl Compiler {
    pub const fn to_flag(self) -> &'static str {
        match self {
            Compiler::Cranelift => "--cranelift",
            Compiler::LLVM => "--llvm",
            Compiler::Singlepass => "--singlepass",
        }
    }
}

pub fn run_code(
    operating_dir: &Path,
    executable_path: &Path,
    args: &[String],
    stderr: bool,
) -> anyhow::Result<String> {
    let output = Command::new(executable_path.canonicalize()?)
        .current_dir(operating_dir)
        .args(args)
        .output()?;

    if !output.status.success() && !stderr {
        bail!(
            "running executable failed: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    let output = std::str::from_utf8(if stderr {
        &output.stderr
    } else {
        &output.stdout
    })
    .expect("output from running executable is not utf-8");

    Ok(output.to_owned())
}

// Take the wasmer/package directory and package it to a .tar.gz tarball
pub fn package_wasmer_to_tarball(tmp_targz_path: &PathBuf) {
    if tmp_targz_path.exists() {
        return;
    }
    let root_path = get_repo_root_path().unwrap();
    let package_path = root_path.join("package");
    if !package_path.exists() {
        panic!("package path {} does not exist", package_path.display());
    }
    println!(
        "packaging /package to .tar.gz: {}",
        tmp_targz_path.display()
    );
    package_directory(&package_path, &tmp_targz_path);
    println!("packaging done");
    println!(
        "tmp tar gz path: {} - exists: {:?}",
        tmp_targz_path.display(),
        tmp_targz_path.exists()
    );
}

fn package_directory(in_dir: &PathBuf, out: &PathBuf) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    let tar = File::create(out).unwrap();
    let enc = GzEncoder::new(tar, Compression::none());
    let mut a = tar::Builder::new(enc);
    a.append_dir_all("bin", in_dir.join("bin")).unwrap();
    a.append_dir_all("lib", in_dir.join("lib")).unwrap();
    a.append_dir_all("include", in_dir.join("include")).unwrap();
    a.finish().unwrap();
}
