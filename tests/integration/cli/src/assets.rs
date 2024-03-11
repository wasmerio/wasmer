use std::path::PathBuf;
use std::{env, path::Path};

pub fn c_asset_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../lib/c-api/examples/assets/"
    ))
}

pub fn asset_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/examples/"
    ))
}

pub fn wasmer_include_path() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../lib/c-api/"))
}

pub fn wasmer_target_path() -> &'static Path {
    let path = if cfg!(feature = "debug") {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/debug/")
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/release/")
    };
    Path::new(path)
}

pub fn wasmer_target_path_2() -> &'static Path {
    let path = if cfg!(feature = "debug") {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../target/",
            env!("CARGO_BUILD_TARGET"),
            "/debug/"
        )
    } else {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../target/",
            env!("CARGO_BUILD_TARGET"),
            "/release/"
        )
    };
    Path::new(path)
}

/* env var TARGET is set by tests/integration/cli/build.rs on compile-time */

pub const LIBWASMER_FILENAME: &str = {
    if cfg!(windows) {
        "wasmer.lib"
    } else {
        "libwasmer.a"
    }
};

/// Get the path to the `libwasmer.a` static library.
pub fn get_libwasmer_path() -> PathBuf {
    let mut ret = env::var("WASMER_TEST_LIBWASMER_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| wasmer_target_path().join(LIBWASMER_FILENAME));

    if !ret.exists() {
        ret = wasmer_target_path_2().join(LIBWASMER_FILENAME);
    }
    if !ret.exists() {
        panic!("Could not find libwasmer path! {:?}", ret);
    }
    ret
}

/// Get the path to the `wasmer` executable to be used in this test.
pub fn get_wasmer_path() -> PathBuf {
    let mut ret = env::var("WASMER_TEST_WASMER_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| wasmer_target_path().join("wasmer"));

    if !ret.exists() {
        ret = wasmer_target_path_2().join("wasmer");
    }
    if !ret.exists() {
        ret = match get_repo_root_path() {
            Some(s) => {
                let release_dir = s.join("target").join("release");

                if cfg!(windows) {
                    release_dir.join("wasmer.exe")
                } else {
                    release_dir.join("wasmer")
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
                let executable = if cfg!(windows) {
                    "wasmer.exe"
                } else {
                    "wasmer"
                };
                s.join("target")
                    .join(target_lexicon::HOST.to_string())
                    .join("release")
                    .join(executable)
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
        panic!(
            "cannot find wasmer / wasmer.exe for integration test at '{}'!",
            ret.display()
        );
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

pub fn get_wasmer_dir() -> Result<PathBuf, anyhow::Error> {
    if let Ok(s) = std::env::var("WASMER_DIR") {
        Ok(Path::new(&s).to_path_buf())
    } else if let Some(root_dir) = get_repo_root_path().and_then(|root| {
        if root.join("package").exists() {
            Some(root.join("package"))
        } else {
            None
        }
    }) {
        Ok(root_dir)
    } else {
        let home_dir = dirs::home_dir()
            .ok_or(anyhow::anyhow!("no home dir"))?
            .join(".wasmer");
        if home_dir.exists() {
            Ok(home_dir)
        } else {
            Err(anyhow::anyhow!("no .wasmer home dir"))
        }
    }
}
