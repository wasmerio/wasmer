use std::env;
use std::path::PathBuf;

pub const C_ASSET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../lib/c-api/examples/assets/"
);
pub const ASSET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/examples/");

pub const WASMER_INCLUDE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../lib/c-api/");

#[cfg(feature = "debug")]
pub const WASMER_TARGET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/debug/");
#[cfg(feature = "debug")]
pub const WASMER_TARGET_PATH_2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/",
    env!("CARGO_BUILD_TARGET"),
    "/debug/"
);

/* env var TARGET is set by tests/integration/cli/build.rs on compile-time */

#[cfg(not(feature = "debug"))]
pub const WASMER_TARGET_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/release/");
#[cfg(not(feature = "debug"))]
pub const WASMER_TARGET_PATH2: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/",
    env!("CARGO_BUILD_TARGET"),
    "/release/"
);

#[cfg(not(windows))]
pub const LIBWASMER_FILENAME: &str = "libwasmer.a";

#[cfg(windows)]
pub const LIBWASMER_FILENAME: &str = "wasmer.lib";

/// Get the path to the `libwasmer.a` static library.
pub fn get_libwasmer_path() -> PathBuf {
    let mut ret = PathBuf::from(
        env::var("WASMER_TEST_LIBWASMER_PATH")
            .unwrap_or_else(|_| format!("{}{}", WASMER_TARGET_PATH, LIBWASMER_FILENAME)),
    );
    if !ret.exists() {
        ret = PathBuf::from(format!("{}{}", WASMER_TARGET_PATH2, LIBWASMER_FILENAME));
    }
    if !ret.exists() {
        panic!("Could not find libwasmer path! {:?}", ret);
    }
    ret
}

/// Get the path to the `wasmer` executable to be used in this test.
pub fn get_wasmer_path() -> PathBuf {
    let mut ret = PathBuf::from(
        env::var("WASMER_TEST_WASMER_PATH")
            .unwrap_or_else(|_| format!("{}wasmer", WASMER_TARGET_PATH)),
    );
    if !ret.exists() {
        ret = PathBuf::from(format!("{}wasmer", WASMER_TARGET_PATH2));
    }
    if !ret.exists() {
        ret = match get_repo_root_path() {
            Some(s) => {
                #[cfg(target_os = "windows")]
                {
                    s.join("target").join("release").join("wasmer.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    s.join("target").join("release").join("wasmer")
                }
            }
            None => {
                panic!("Could not find wasmer executable path! {:?}", ret);
            }
        };
    }

    if !ret.exists() {
        ret = match get_repo_root_path() {
            Some(s) => {
                #[cfg(target_os = "windows")]
                {
                    s.join("target")
                        .join(target_lexicon::HOST.to_string())
                        .join("release")
                        .join("wasmer.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    s.join("target")
                        .join(target_lexicon::HOST.to_string())
                        .join("release")
                        .join("wasmer")
                }
            }
            None => {
                panic!("Could not find wasmer executable path! {:?}", ret);
            }
        };
    }

    if !ret.exists() {
        if let Some(root) = get_repo_root_path() {
            use std::process::Stdio;
            let _ = std::process::Command::new("ls")
                .arg(root.join("target"))
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .stdin(Stdio::null())
                .output();
        }
        panic!("cannot find wasmer / wasmer.exe for integration test!");
    }
    ret
}

pub fn get_repo_root_path() -> Option<PathBuf> {
    let mut current_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut counter = 0;
    let mut result = None;
    'outer: while counter < 50 {
        counter += 1;
        if current_dir.join("CHANGELOG.md").exists() && current_dir.join("LICENSE").exists() {
            result = Some(current_dir.to_path_buf());
            break 'outer;
        } else {
            current_dir = current_dir.parent()?;
        }
    }
    result
}
