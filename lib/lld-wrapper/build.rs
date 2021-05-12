use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use pathsearch::find_executable_in_path;

// We try to parse these the same as the llvm-sys crate to simplify setup of a
// single llvm+lld.
macro_rules! llvm_version {
    () => {
        "11"
    };
}
macro_rules! llvm_sys_llvm_prefix {
    () => {
        concat!("LLVM_SYS_", llvm_version!(), "_PREFIX")
    };
}
macro_rules! llvm_sys_strict_versioning {
    () => {
        concat!("LLVM_SYS_", llvm_version!(), "_STRICT_VERSIONING")
    };
}

fn is_compatible_llvm_config(llvm_config: &Path) -> bool {
    let output = Command::new(llvm_config).arg("--version").output();
    if output.is_err() {
        return false;
    }
    let output = output.unwrap();
    if !output.status.success() {
        return false;
    }
    let output = output.stdout;
    let strict_match = output.starts_with(concat!(llvm_version!(), ".").as_bytes());
    if strict_match {
        return true;
    }
    if cfg!(feature = "strict-versioning") || std::env::var(llvm_sys_strict_versioning!()).is_ok() {
        return false;
    }

    let re = regex::bytes::Regex::new(r"^(\d+)\.\d+(?:\.\d+)?").unwrap();
    return re.captures(&output).map_or(false, |c| {
        c.len() == 1
            && c.get(0).map_or(false, |major_cstr| {
                std::str::from_utf8(major_cstr.as_bytes()).map_or(false, |major_str| {
                    major_str.parse::<u32>().map_or(false, |major| {
                        major > llvm_version!().parse::<u32>().unwrap()
                    })
                })
            })
    });
}

fn find_llvm_config() -> Option<PathBuf> {
    let llvm_configs = vec![
        "llvm-config",
        concat!("llvm-config-", llvm_version!()),
        concat!("llvm-config-", llvm_version!(), ".2"),
        concat!("llvm-config", llvm_version!(), "2"),
        concat!("llvm-config-", llvm_version!(), ".1"),
        concat!("llvm-config", llvm_version!(), "1"),
        concat!("llvm-config-", llvm_version!(), ".0"),
        concat!("llvm-config", llvm_version!(), "0"),
    ];

    if let Ok(path) = std::env::var(llvm_sys_llvm_prefix!()) {
        let path = Path::new(path.as_str());
        let mut pathbuf = path.to_path_buf();
        pathbuf.push("bin");
        for llvm_config in &llvm_configs {
            let mut pb = pathbuf.clone();
            pb.push(llvm_config);

            if is_compatible_llvm_config(&pb) {
                return Some(pb);
            }
        }
        return None;
    }

    for llvm_config in llvm_configs {
        if let Some(exe) = find_executable_in_path(llvm_config) {
            if is_compatible_llvm_config(&exe) {
                return Some(exe);
            }
        }
    }

    return None;
}

fn run_llvm_config(llvm_config: &Path, args: &[&OsStr]) -> Vec<u8> {
    let mut output = Command::new(&llvm_config)
        .args(args)
        .output()
        .unwrap()
        .stdout;
    if output.last() == Some(&b'\n') {
        output.pop();
        if output.last() == Some(&b'\r') {
            output.pop();
        }
    }
    output
}

fn main() {
    let llvm_config = match find_llvm_config() {
        None => {
            println!("Didn't find a usable llvm-config.");
            return;
        }
        Some(x) => x,
    };
    dbg!(&llvm_config);

    // TODO: improve compatibility with llvm-sys
    //
    // --cflags:
    //   llvm-sys sets CFLAGS environment variable before running cc::Build
    //   removes -W flags except on msvc or LLVM_SYS_{}_NO_CLEAN_CFLAGS is set
    // --build-mode:
    //   llvm-sys has an incorrect command about there only being Debug and
    //   Release build modes, there's also RelWithDebInfo, MinSizeRel. All it
    //   does is on msvc with Debug llvm (or LLVM_SYS_{}_USE_DEBUG_MSVCRT)
    //   then link with msvcrtd.

    let libdir = run_llvm_config(&llvm_config, &[OsStr::new("--libdir")]);

    let incdir = run_llvm_config(&llvm_config, &[OsStr::new("--includedir")]);
    let incdir = Path::new::<OsStr>(OsStrExt::from_bytes(&incdir));

    let libs = run_llvm_config(
        &llvm_config,
        &[OsStr::new("--link-static"), OsStr::new("--libnames")],
    );
    let libs = libs
        .split(|c| *c == b' ' || *c == b'\n')
        .filter(|l| !l.is_empty())
        .map(|mut lib| {
            if lib.starts_with(b"lib") {
                lib = lib.split_at(3).1;
            }
            if lib.ends_with(b".a") {
                lib = lib.split_at(lib.len() - 2).0;
            }
            lib
        })
        .map(OsStrExt::from_bytes)
        .collect::<Vec<&OsStr>>();

    let system_libs = run_llvm_config(
        &llvm_config,
        &[OsStr::new("--link-static"), OsStr::new("--system-libs")],
    );
    let system_libs = system_libs
        .split(|c| *c == b' ' || *c == b'\n')
        .filter(|l| !l.is_empty())
        .map(|mut lib| {
            if lib.starts_with(b"-l") {
                lib = lib.split_at(2).1;
            }
            lib
        })
        .map(OsStrExt::from_bytes)
        .collect::<Vec<&OsStr>>();

    println!("cargo:rerun-if-changed=src/link.cc");
    println!("cargo:rerun-if-env-changed={}", llvm_sys_llvm_prefix!());
    println!(
        "cargo:rerun-if-env-changed={}",
        llvm_sys_strict_versioning!()
    );
    std::io::stdout()
        .write_all(b"cargo:rustc-link-search=")
        .unwrap();
    std::io::stdout().write_all(&libdir).unwrap();
    std::io::stdout().write_all(b"\n").unwrap();

    // These are not included in llvm-config, they're lld libraries.
    println!("cargo:rustc-link-lib=lldELF");
    println!("cargo:rustc-link-lib=lldCommon");

    for lib in libs {
        std::io::stdout()
            .write_all(b"cargo:rustc-link-lib=")
            .unwrap();
        std::io::stdout().write_all(lib.as_bytes()).unwrap();
        std::io::stdout().write_all(b"\n").unwrap();
    }

    for lib in system_libs {
        std::io::stdout()
            .write_all(b"cargo:rustc-link-lib=dylib=")
            .unwrap();
        std::io::stdout().write_all(lib.as_bytes()).unwrap();
        std::io::stdout().write_all(b"\n").unwrap();
    }

    cc::Build::new()
        .cpp(true)
        .file("src/link.cc")
        .include(Path::new(&incdir))
        .compile("liblink.a");
}
