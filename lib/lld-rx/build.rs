use std::env;
use std::ffi::OsStr;
use std::io::{self};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

static LLVM_CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(prefix) = env::var_os("LLVM_SYS_221_PREFIX") {
        Path::new(&prefix).join("bin").join("llvm-config")
    } else {
        which::which("llvm-config").expect("llvm-config cannot be found")
    }
});

fn target_env_is(name: &str) -> bool {
    match env::var_os("CARGO_CFG_TARGET_ENV") {
        Some(s) => s == name,
        None => false,
    }
}

fn target_os_is(name: &str) -> bool {
    match env::var_os("CARGO_CFG_TARGET_OS") {
        Some(s) => s == name,
        None => false,
    }
}

fn llvm_config(arg: &str) -> String {
    llvm_config_ex(&*LLVM_CONFIG_PATH, arg).expect("Surprising failure from llvm-config")
}

fn llvm_config_ex<S: AsRef<OsStr>>(binary: S, arg: &str) -> io::Result<String> {
    Command::new(binary)
        .arg(arg)
        .arg("--link-static")
        // .arg("core") // We do, in fact need everything!
        .output()
        .and_then(|output| {
            if output.stdout.is_empty() {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "llvm-config returned empty output",
                ))
            } else {
                Ok(String::from_utf8(output.stdout)
                    .expect("Output from llvm-config was not valid UTF-8"))
            }
        })
}

fn get_system_libraries() -> Vec<String> {
    llvm_config("--system-libs")
        .split(&[' ', '\n'] as &[char])
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("/"))
        .map(|flag| {
            if target_env_is("msvc") {
                assert!(
                    flag.ends_with(".lib"),
                    "system library {:?} does not appear to be a MSVC library file",
                    flag
                );
                &flag[..flag.len() - 4]
            } else {
                if let Some(striped) = flag.strip_prefix("-l") {
                    if target_os_is("macos") && flag.starts_with("-llib") && flag.ends_with(".tbd")
                    {
                        return flag[5..flag.len() - 4].to_owned();
                    }
                    return striped.to_owned();
                }

                let maybe_lib = Path::new(&flag);
                if maybe_lib.is_file() {
                    println!(
                        "cargo:rustc-link-search={}",
                        maybe_lib.parent().unwrap().display()
                    );

                    let soname = maybe_lib
                        .file_name()
                        .unwrap()
                        .to_str()
                        .expect("Shared library path must be a valid string");
                    let stem = soname
                        .rsplit_once(target_dylib_extension())
                        .expect("Shared library should be a .so file")
                        .0;

                    stem.trim_start_matches("lib")
                } else {
                    panic!(
                        "Unable to parse result of llvm-config --system-libs: was {:?}",
                        flag
                    )
                }
            }
            .to_owned()
        })
        .chain(get_system_libcpp().map(str::to_owned))
        .collect::<Vec<String>>()
}

fn target_dylib_extension() -> &'static str {
    if target_os_is("macos") {
        ".dylib"
    } else {
        ".so"
    }
}

fn get_system_libcpp() -> Option<&'static str> {
    if target_env_is("msvc") {
        None
    } else if target_os_is("macos") || target_os_is("freebsd") {
        Some("c++")
    } else {
        Some("stdc++")
    }
}

fn get_link_libraries() -> Vec<String> {
    llvm_config("--libnames")
        .split(&[' ', '\n'] as &[char])
        .filter(|s| !s.is_empty())
        .map(|name| {
            if target_env_is("msvc") {
                assert!(
                    name.ends_with(".lib"),
                    "library name {:?} does not appear to be a MSVC library file",
                    name
                );
                &name[..name.len() - 4]
            } else {
                assert!(
                    name.starts_with("lib") && name.ends_with(".a"),
                    "library name {:?} does not appear to be a static library",
                    name
                );
                &name[3..name.len() - 2]
            }
        })
        .map(str::to_owned)
        .collect::<Vec<String>>()
}

fn get_llvm_cxxflags() -> String {
    let output = llvm_config("--cxxflags");

    let no_clean = env::var_os(format!(
        "LLVM_SYS_{}_NO_CLEAN_CFLAGS",
        env!("CARGO_PKG_VERSION_MAJOR")
    ))
    .is_some();
    if no_clean || target_env_is("msvc") {
        return output;
    }

    llvm_config("--cxxflags")
        .split(&[' ', '\n'][..])
        .filter(|word| !word.starts_with("-W"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_llvm_debug() -> bool {
    llvm_config("--build-mode").contains("Debug")
}

fn main() {
    // SAFETY: called in a single-threaded context
    unsafe {
        env::set_var("CXXFLAGS", get_llvm_cxxflags());
    }
    let mut build = cc::Build::new();

    build.cpp(true).file("wrapper/library.cpp");

    if target_os_is("linux") {
        build.define("LLD_RX_HAS_ELF_DRIVER", Some("1"));
    } else if target_os_is("macos") {
        build.define("LLD_RX_HAS_MACHO_DRIVER", Some("1"));
    } else if target_os_is("windows") {
        build.define("LLD_RX_HAS_COFF_DRIVER", Some("1"));
    }

    if build.get_compiler().is_like_msvc() {
        build.flag("/std:c++20");
    } else {
        build.flag("-std=c++20");
    }

    build.compile("lldwrapper");

    println!("cargo:rerun-if-changed=wrapper/library.cpp");
    println!("cargo:rerun-if-env-changed=DEP_LLVM_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");

    let libdir = llvm_config("--libdir");

    println!("cargo:config_path={}", LLVM_CONFIG_PATH.display());
    println!("cargo:libdir={}", libdir);

    println!("cargo:rustc-link-search=native={}", libdir);
    let blacklist = ["LLVMLineEditor"];
    for name in get_link_libraries()
        .iter()
        .filter(|n| !blacklist.iter().any(|blacklisted| n.contains(*blacklisted)))
    {
        println!("cargo:rustc-link-lib=static={}", name);
    }

    for name in get_system_libraries() {
        println!("cargo:rustc-link-lib=dylib={}", name);
    }

    let use_debug_msvcrt = env::var_os(format!(
        "LLVM_SYS_{}_USE_DEBUG_MSVCRT",
        env!("CARGO_PKG_VERSION_MAJOR")
    ))
    .is_some();
    if cfg!(target_env = "msvc") && (use_debug_msvcrt || is_llvm_debug()) {
        println!("cargo:rustc-link-lib=msvcrtd");
    }

    println!("cargo:rustc-link-lib=static=lldCommon");
    // Special LLD libraries!
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=static=lldELF");
    } else if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=static=lldMachO");
    } else if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=static=lldCOFF");
        println!("cargo:rustc-link-lib=static=lldMinGW");
    }

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=xar");
    }

    if cfg!(not(target_os = "windows")) {
        println!("cargo:rustc-link-lib=dylib=ffi");
    }
}
