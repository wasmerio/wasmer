//! This file was mostly taken from the llvm-sys crate.
//! (https://bitbucket.org/tari/llvm-sys.rs/raw/94361c1083a88f439b9d24c59b2d2831517413d7/build.rs)

use futures::executor::block_on;
use lazy_static::lazy_static;
use regex::Regex;
use semver::Version;
#[cfg(not(target_os = "windows"))]
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "windows"))]
use std::convert::TryInto;
use std::env;
use std::ffi::OsStr;
#[cfg(not(target_os = "windows"))]
use std::fs::File;
use std::io::Write;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::process::Command;

#[cfg(not(target_os = "windows"))]
#[macro_use]
extern crate hex_literal;

// Version of the llvm-sys crate that we (through inkwell) depend on.
const LLVM_SYS_MAJOR_VERSION: &str = "80";
const LLVM_SYS_MINOR_VERSION: &str = "0";

// Environment variables that can guide compilation
//
// When adding new ones, they should also be added to main() to force a
// rebuild if they are changed.
lazy_static! {

    /// A single path to search for LLVM in (containing bin/llvm-config)
    static ref ENV_LLVM_PREFIX: String =
        format!("LLVM_SYS_{}_PREFIX", LLVM_SYS_MAJOR_VERSION);

    /// If exactly "YES", ignore the version blacklist
    static ref ENV_IGNORE_BLACKLIST: String =
        format!("LLVM_SYS_{}_IGNORE_BLACKLIST", LLVM_SYS_MAJOR_VERSION);

    /// If set, enforce precise correspondence between crate and binary versions.
    static ref ENV_STRICT_VERSIONING: String =
        format!("LLVM_SYS_{}_STRICT_VERSIONING", LLVM_SYS_MAJOR_VERSION);

    /// If set, do not attempt to strip irrelevant options for llvm-config --cflags
    static ref ENV_NO_CLEAN_CXXFLAGS: String =
        format!("LLVM_SYS_{}_NO_CLEAN_CXXFLAGS", LLVM_SYS_MAJOR_VERSION);

    /// If set and targeting MSVC, force the debug runtime library
    static ref ENV_USE_DEBUG_MSVCRT: String =
        format!("LLVM_SYS_{}_USE_DEBUG_MSVCRT", LLVM_SYS_MAJOR_VERSION);

    /// If set, always link against libffi
    static ref ENV_FORCE_FFI: String =
        format!("LLVM_SYS_{}_FFI_WORKAROUND", LLVM_SYS_MAJOR_VERSION);

    /// If set, disable the automatic LLVM installing
    static ref ENV_DISABLE_INSTALL: String =
        format!("LLVM_SYS_{}_DISABLE_INSTALL", LLVM_SYS_MAJOR_VERSION);

    /// LLVM version used by this version of the crate.
    static ref CRATE_VERSION: Version = {
        Version::new(LLVM_SYS_MAJOR_VERSION.parse::<u64>().unwrap() / 10,
                     LLVM_SYS_MINOR_VERSION.parse::<u64>().unwrap() % 10,
                     0)
    };

    /// LLVM possible names for the binary files
    static ref LLVM_CONFIG_BINARY_NAMES: Vec<String> = {
        vec![
            "llvm-config".into(),
            format!("llvm-config-{}", CRATE_VERSION.major),
            format!("llvm-config-{}.{}", CRATE_VERSION.major, CRATE_VERSION.minor),
            format!("llvm-config{}{}", CRATE_VERSION.major, CRATE_VERSION.minor),
        ]
    };

    static ref LLVM_OUT_PATH: String =
        format!("{}/llvm", std::env::var("OUT_DIR").unwrap());

    static ref LLVM_OUT_TARGET_PATH: String =
        format!("{}/{}", *LLVM_OUT_PATH, llvm_target_name());
}

fn get_llvm_binary_from_path(path: &PathBuf) -> Option<PathBuf> {
    for binary_name in LLVM_CONFIG_BINARY_NAMES.iter() {
        let mut pb: PathBuf = path.clone().into();
        pb.push("bin");
        pb.push(binary_name);

        let ver = llvm_version(&pb).expect(&format!("Failed to execute {:?}", &pb));
        if is_compatible_llvm(&ver) {
            return Some(pb);
        } else {
            println!(
                "LLVM binaries specified by {} are the wrong version.
                        (Found {}, need {}.)",
                *ENV_LLVM_PREFIX, ver, *CRATE_VERSION
            );
        }
    }
    None
}

fn locate_user_llvm_config() -> Option<PathBuf> {
    println!("Locating user-provided preinstalled LLVM");
    // Did the user give us a binary path to use? If yes, try
    // to use that and fail if it doesn't work.
    if let Some(env_var) = env::var_os(&*ENV_LLVM_PREFIX) {
        println!("Environment var found: {}={:?}", *ENV_LLVM_PREFIX, env_var);
        match get_llvm_binary_from_path(&(env_var.clone().into())) {
            Some(path) => return Some(path.into()),
            None => {
                println!(
                    "No suitable version of LLVM was found pointed to by {}.",
                    *ENV_LLVM_PREFIX
                );
            }
        }
    }
    None
}

fn locate_wasmer_preinstalled_llvm_config() -> Option<PathBuf> {
    println!(
        "Locating Wasmer preinstalled LLVM at {}",
        *LLVM_OUT_TARGET_PATH
    );
    match get_llvm_binary_from_path(&(*LLVM_OUT_TARGET_PATH).clone().into()) {
        Some(path) => return Some(path.into()),
        None => {
            println!(
                "No suitable version of LLVM was found pointed to by {}.",
                *ENV_LLVM_PREFIX
            );
        }
    }
    None
}

/// Try to find a system-wide version of llvm-config that is compatible with
/// this crate.
///
/// Returns None on failure.
fn locate_system_llvm_config() -> Option<PathBuf> {
    println!("Locating System provided LLVM");
    for binary_name in LLVM_CONFIG_BINARY_NAMES.iter() {
        match llvm_version(binary_name) {
            Ok(ref version) if is_compatible_llvm(version) => {
                // Compatible version found. Nice.
                return Some(binary_name.into());
            }
            Ok(version) => {
                // Version mismatch. Will try further searches, but warn that
                // we're not using the system one.
                println!(
                    "Found LLVM version {} on PATH, but need {}.",
                    version, *CRATE_VERSION
                );
            }
            Err(ref e) if e.kind() == ErrorKind::NotFound => {
                // Looks like we failed to execute any llvm-config. Keep
                // searching.
            }
            // Some other error, probably a weird failure. Give up.
            Err(e) => panic!("Failed to search PATH for llvm-config: {}", e),
        }
    }

    None
}

/// Check whether the given version of LLVM is blacklisted,
/// returning `Some(reason)` if it is.
fn is_blacklisted_llvm(llvm_version: &Version) -> Option<&'static str> {
    static BLACKLIST: &'static [(u64, u64, u64, &'static str)] = &[];

    if let Some(x) = env::var_os(&*ENV_IGNORE_BLACKLIST) {
        if &x == "YES" {
            println!(
                "cargo:warning=Ignoring blacklist entry for LLVM {}",
                llvm_version
            );
            return None;
        } else {
            println!(
                "cargo:warning={} is set but not exactly \"YES\"; blacklist is still honored.",
                *ENV_IGNORE_BLACKLIST
            );
        }
    }

    for &(major, minor, patch, reason) in BLACKLIST.iter() {
        let bad_version = Version {
            major: major,
            minor: minor,
            patch: patch,
            pre: vec![],
            build: vec![],
        };

        if &bad_version == llvm_version {
            return Some(reason);
        }
    }
    None
}

/// Check whether the given LLVM version is compatible with this version of
/// the crate.
fn is_compatible_llvm(llvm_version: &Version) -> bool {
    if let Some(reason) = is_blacklisted_llvm(llvm_version) {
        println!(
            "Found LLVM {}, which is blacklisted: {}",
            llvm_version, reason
        );
        return false;
    }

    let strict =
        env::var_os(&*ENV_STRICT_VERSIONING).is_some() || cfg!(feature = "strict-versioning");
    if strict {
        llvm_version.major == CRATE_VERSION.major && llvm_version.minor == CRATE_VERSION.minor
    } else {
        llvm_version.major >= CRATE_VERSION.major
            || (llvm_version.major == CRATE_VERSION.major
                && llvm_version.minor >= CRATE_VERSION.minor)
    }
}

/// Invoke the specified binary as llvm-config.
///
/// Explicit version of the `llvm_config` function that bubbles errors
/// up.
fn llvm_config<S: AsRef<OsStr>>(binary: S, arg: &str) -> io::Result<String> {
    Command::new(binary)
        .arg(arg)
        .arg("--link-static") // Don't use dylib for >= 3.9
        .output()
        .map(|output| {
            String::from_utf8(output.stdout).expect("Output from llvm-config was not valid UTF-8")
        })
}

/// Get the LLVM version using llvm-config.
fn llvm_version<S: AsRef<OsStr>>(binary: S) -> io::Result<Version> {
    let version_str = llvm_config(binary.as_ref(), "--version")?;

    // LLVM isn't really semver and uses version suffixes to build
    // version strings like '3.8.0svn', so limit what we try to parse
    // to only the numeric bits.
    let re = Regex::new(r"^(?P<major>\d+)\.(?P<minor>\d+)(?:\.(?P<patch>\d+))??").unwrap();
    let c = re
        .captures(&version_str)
        .expect("Could not determine LLVM version from llvm-config.");

    // some systems don't have a patch number but Version wants it so we just append .0 if it isn't
    // there
    let s = match c.name("patch") {
        None => format!("{}.0", &c[0]),
        Some(_) => c[0].to_string(),
    };
    Ok(Version::parse(&s).unwrap())
}

fn get_llvm_cxxflags<S: AsRef<OsStr>>(binary: S) -> String {
    let output = llvm_config(binary, "--cxxflags").expect("Can't get cxx flags");

    // llvm-config includes cflags from its own compilation with --cflags that
    // may not be relevant to us. In particularly annoying cases, these might
    // include flags that aren't understood by the default compiler we're
    // using. Unless requested otherwise, clean CFLAGS of options that are
    // known to be possibly-harmful.
    let no_clean = env::var_os(&*ENV_NO_CLEAN_CXXFLAGS).is_some();
    if no_clean || cfg!(target_env = "msvc") {
        // MSVC doesn't accept -W... options, so don't try to strip them and
        // possibly strip something that should be retained. Also do nothing if
        // the user requests it.
        return output;
    }

    output
        .split(&[' ', '\n'][..])
        .filter(|word| !word.starts_with("-W"))
        .filter(|word| word != &"-fno-exceptions")
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_llvm_debug<S: AsRef<OsStr>>(binary: S) -> bool {
    // Has to be either Debug or Release
    llvm_config(binary, "--build-mode")
        .expect("Can't get LLVM build mode")
        .contains("Debug")
}

#[cfg(not(target_os = "windows"))]
fn llvm_target_name() -> String {
    let name = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-linux-gnu-ubuntu-16.04"
    } else {
        panic!("Unsupported host for installing llvm")
    };

    format!(
        "clang+llvm-{}.{}.0-{}",
        CRATE_VERSION.major, CRATE_VERSION.minor, name
    )
}

#[cfg(not(target_os = "windows"))]
fn llvm_url() -> String {
    let name = llvm_target_name();
    format!(
        "https://releases.llvm.org/{}.{}.0/{}.tar.xz",
        CRATE_VERSION.major, CRATE_VERSION.minor, name
    )
}

#[cfg(not(target_os = "windows"))]
async fn download_llvm_binary(download_path: &PathBuf) -> io::Result<()> {
    if download_path.exists() {
        return Ok(());
    }

    let url = llvm_url();
    println!("Downloading LLVM from: {}", url);
    let mut resp = surf::get(&url)
        .await
        .expect("Failed to connect to the llvm server");
    let mut bytes = resp.body_bytes().await.expect("can't get bytes");
    let mut out = File::create(download_path)?;

    out.write_all(&bytes);

    if !verify_sha256sum(download_path) {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Failed to verify downloaded file by sha256sum",
        ));
    };
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn verify_sha256sum(download_path: &PathBuf) -> bool {
    let mut llvm_file = File::open(download_path).expect("Failed to open downloaded llvm file");
    let mut sha256 = Sha256::new();
    io::copy(&mut llvm_file, &mut sha256).expect("Failed to input data to sha256 hasher");

    let is_verify = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        // Generated by `shasum -a 256 clang+llvm-8.0.0-x86_64-apple-darwin.tar.xz`
        let sha256sum_macos =
            hex!("94ebeb70f17b6384e052c47fef24a6d70d3d949ab27b6c83d4ab7b298278ad6f");
        if sha256.result().try_into() == Ok(sha256sum_macos) {
            true
        } else {
            false
        }
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        // Generated by `sha256sum clang+llvm-8.0.0-x86_64-linux-gnu-ubuntu-16.04.tar.xz`
        let sha256sum_linux =
            hex!("87b88d620284d1f0573923e6f7cc89edccf11d19ebaec1cfb83b4f09ac5db09c");
        if sha256.result().try_into() == Ok(sha256sum_linux) {
            true
        } else {
            false
        }
    } else {
        false
    };

    is_verify
}

fn set_env_llvm_prefix_to_user_installed_llvm() {
    println!(
        "Setting environment var {}={:?}",
        *ENV_LLVM_PREFIX, *LLVM_OUT_TARGET_PATH
    );
    std::env::set_var(&*ENV_LLVM_PREFIX, &*LLVM_OUT_TARGET_PATH);
}

#[cfg(not(target_os = "windows"))]
fn install_llvm() {
    use xz2::read::XzDecoder;

    let llvm_path: PathBuf = LLVM_OUT_PATH.clone().into();
    let full_llvm_path: PathBuf = LLVM_OUT_TARGET_PATH.clone().into();

    println!("Installing LLVM to {:?}", full_llvm_path);

    if full_llvm_path.exists() {
        println!("LLVM already installed at {:?}", full_llvm_path);
        set_env_llvm_prefix_to_user_installed_llvm();
        return;
    }

    let mut download_path = llvm_path.clone();
    download_path.set_file_name(format!("{}.tar.xz", llvm_target_name()));

    if !download_path.exists() {
        println!("Downloading LLVM to: {:?}", download_path);
        let future = download_llvm_binary(&download_path);
        block_on(future).expect("Failed to download llvm binary");
    }

    println!(
        "Uncompressing LLVM from {:?} to {:?}",
        download_path, llvm_path
    );
    let llvm_file = File::open(&download_path).expect("Failed to open downloaded llvm file");
    let lzma_reader = XzDecoder::new(llvm_file);
    let mut archive = tar::Archive::new(lzma_reader);

    archive
        .unpack(&llvm_path)
        .expect("Failed to unpack the tar file");

    set_env_llvm_prefix_to_user_installed_llvm();
}

fn main() {
    println!("cargo:rustc-link-lib=static=llvm-backend");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cpp/object_loader.cpp");
    println!("cargo:rerun-if-changed=cpp/object_loader.hh");
    println!("cargo:rerun-if-env-changed={}", &*ENV_LLVM_PREFIX);
    println!("cargo:rerun-if-env-changed={}", &*ENV_IGNORE_BLACKLIST);
    println!("cargo:rerun-if-env-changed={}", &*ENV_STRICT_VERSIONING);
    println!("cargo:rerun-if-env-changed={}", &*ENV_NO_CLEAN_CXXFLAGS);
    println!("cargo:rerun-if-env-changed={}", &*ENV_USE_DEBUG_MSVCRT);
    println!("cargo:rerun-if-env-changed={}", &*ENV_FORCE_FFI);
    println!("cargo:rerun-if-env-changed={}", &*ENV_DISABLE_INSTALL);

    let system_llvm_config = locate_system_llvm_config();
    // Install LLVM automatically only if the following conditions matches:
    //   - Your environment is Mac OS or Linux.
    //   - The system LLVM doesn't exist.
    //   - Internet connection exists.
    //   - The flag to disable installing isn't set.
    #[cfg(not(target_os = "windows"))]
    {
        let should_disable_install = match env::var_os(&*ENV_DISABLE_INSTALL) {
            Some(v) => v != "1",
            None => false,
        };
        let is_online = online::online(None).is_ok();
        let user_provided_llvm = env::var_os(&*ENV_LLVM_PREFIX).is_some();
        let has_system_llvm = false;
        // let has_system_llvm = system_llvm_config.is_some();

        if !should_disable_install && is_online && !user_provided_llvm && !has_system_llvm {
            install_llvm();
        }
    }

    let llvm_config = locate_user_llvm_config()
        .or_else(locate_system_llvm_config)
        .or_else(locate_wasmer_preinstalled_llvm_config)
        .expect("Couldn't find a usable LLVM");

    std::env::set_var("CXXFLAGS", get_llvm_cxxflags(&llvm_config));
    cc::Build::new()
        .cpp(true)
        .file("cpp/object_loader.cpp")
        .compile("llvm-backend");

    // Enable "nightly" cfg if the current compiler is nightly.
    if rustc_version::version_meta().unwrap().channel == rustc_version::Channel::Nightly {
        println!("cargo:rustc-cfg=nightly");
    }

    let use_debug_msvcrt = env::var_os(&*ENV_USE_DEBUG_MSVCRT).is_some();
    if cfg!(target_env = "msvc") && (use_debug_msvcrt || is_llvm_debug(&llvm_config)) {
        println!("cargo:rustc-link-lib={}", "msvcrtd");
    }

    // Link libffi if the user requested this workaround.
    // See https://bitbucket.org/tari/llvm-sys.rs/issues/12/
    let force_ffi = env::var_os(&*ENV_FORCE_FFI).is_some();
    if force_ffi {
        println!("cargo:rustc-link-lib=dylib={}", "ffi");
    }
}
