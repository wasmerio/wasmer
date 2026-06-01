#[cfg(feature = "v8")]
fn build_v8() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{
        env, fs,
        path::PathBuf,
        sync::{LazyLock, Mutex},
    };

    const WEE8_RELEASE_VERSION: &str = "11.9.6";

    let (asset_name, platform_name) = match (
        env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ENV")
            .unwrap_or_default()
            .as_str(),
    ) {
        ("macos", "aarch64", _) => ("v8-darwin-aarch64.tar.xz", "darwin-aarch64"),
        ("linux", "x86_64", "gnu") => ("v8-linux-amd64.tar.xz", "linux-amd64"),
        ("linux", "x86_64", "musl") => ("v8-linux-musl.tar.xz", "linux-musl"),
        ("windows", "x86_64", _) => ("v8-windows-amd64.tar.xz", "windows-amd64"),
        ("android", "aarch64", _) => ("v8-android-arm64.tar.xz", "android-arm64"),
        (os, arch, _) => panic!("target os + arch combination not supported: {os}, {arch}"),
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = PathBuf::from(&out_dir);
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let v8_header_path = PathBuf::from(&crate_root).join("third-party").join("wee8");
    let cache_dir = out_path
        .join("../../../../")
        .join("wee8-artifacts")
        .join(WEE8_RELEASE_VERSION)
        .join(platform_name);
    let archive_path = cache_dir.join(asset_name);
    let v8_lib_dir = cache_dir.join("lib");
    let v8_lib_path = v8_lib_dir.join(if cfg!(target_os = "windows") {
        "v8.lib"
    } else {
        "libv8.a"
    });

    if !v8_lib_path.exists() {
        fs::create_dir_all(&cache_dir).unwrap_or_else(|err| {
            panic!(
                "failed to create v8 cache dir {}: {err}",
                cache_dir.display()
            )
        });

        if v8_lib_dir.exists() && !v8_lib_path.exists() {
            fs::remove_dir_all(&v8_lib_dir).unwrap_or_else(|err| {
                panic!(
                    "failed to remove incomplete wee8 lib dir {}: {err}",
                    v8_lib_dir.display()
                )
            });
        }

        if !archive_path.exists() {
            use std::io::Write;

            let url = format!(
                "https://github.com/wasmerio/wee8-custom-builds/releases/download/{WEE8_RELEASE_VERSION}/{asset_name}"
            );
            let tar_data = ureq::get(&url)
                .call()
                .expect("failed to download v8")
                .body_mut()
                .with_config()
                // Windows prebuilts are substantially larger.
                .limit(200 * 1024 * 1024)
                .read_to_vec()
                .expect("failed to download v8 lib");

            let mut archive_tmp =
                tempfile::NamedTempFile::new_in(&cache_dir).unwrap_or_else(|err| {
                    panic!(
                        "failed to create temporary v8 archive download file in {}: {err}",
                        cache_dir.display()
                    )
                });
            archive_tmp.write_all(&tar_data).unwrap_or_else(|err| {
                panic!(
                    "failed to write temporary v8 archive download file {}: {err}",
                    archive_tmp.path().display()
                )
            });
            archive_tmp.persist(&archive_path).unwrap_or_else(|err| {
                panic!(
                    "failed to finalize v8 archive cache {} from temporary file {}: {}",
                    archive_path.display(),
                    err.file.path().display(),
                    err.error
                )
            });
        }

        let tar_data = fs::read(&archive_path).unwrap_or_else(|err| {
            panic!(
                "failed to read v8 archive cache {}: {err}",
                archive_path.display()
            )
        });
        let unpack_tmp = tempfile::TempDir::new_in(&cache_dir).unwrap_or_else(|err| {
            panic!(
                "failed to create temporary wee8 unpack directory in {}: {err}",
                cache_dir.display()
            )
        });
        let tar = xz::read::XzDecoder::new(tar_data.as_slice());
        let mut archive = tar::Archive::new(tar);
        archive.unpack(unpack_tmp.path()).unwrap_or_else(|err| {
            let _ = fs::remove_file(&archive_path);
            panic!(
                "failed to unpack v8 archive cache {} into {} (cache removed so the next build can re-download): {err}",
                archive_path.display(),
                unpack_tmp.path().display()
            )
        });
        let staged_lib = unpack_tmp.path().join("lib");
        if !staged_lib.is_dir() {
            let _ = fs::remove_file(&archive_path);
            panic!(
                "v8 archive {} did not contain a top-level `lib/` directory after unpack (expected {})",
                archive_path.display(),
                staged_lib.display()
            );
        }
        if v8_lib_dir.exists() {
            fs::remove_dir_all(&v8_lib_dir).unwrap_or_else(|err| {
                panic!(
                    "failed to remove existing wee8 lib dir {} before rename: {err}",
                    v8_lib_dir.display()
                )
            });
        }
        fs::rename(&staged_lib, &v8_lib_dir).unwrap_or_else(|err| {
            let _ = fs::remove_file(&archive_path);
            panic!(
                "failed to move unpacked wee8 lib from {} to {}: {err}",
                staged_lib.display(),
                v8_lib_dir.display()
            )
        });
    }

    println!("cargo:rustc-link-search=native={}", v8_lib_dir.display());

    if cfg!(any(target_os = "linux",)) {
        println!("cargo:rustc-link-lib=stdc++");
    } else if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=dbghelp");
        println!("cargo:rustc-link-lib=shlwapi");
    } else {
        println!("cargo:rustc-link-lib=c++");
    }

    // Rename the wasm-c-api symbols from V8 so they do not collide with
    // Wasmer's own wasm-c-api exports when this crate is linked into tests.
    static WEE8_RENAMED: LazyLock<Mutex<Vec<(String, String)>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));

    #[derive(Debug)]
    struct Wee8Renamer {}
    impl ParseCallbacks for Wee8Renamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = format!("wee8_{}", item_info.name);
                WEE8_RENAMED
                    .lock()
                    .expect("cannot lock WEE8_RENAMED")
                    .push((item_info.name.to_string(), new_name.clone()));
                Some(new_name)
            } else {
                None
            }
        }
    }

    let header_path = v8_header_path.join("wasm.h");
    let mut args = vec![];
    if cfg!(target_os = "macos") {
        args.push("-I/usr/local/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/include/c++/v1");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/usr/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/include");
        args.push("-I/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/System/Library/Frameworks");
    }
    let bindings = bindgen::Builder::default()
        .header(header_path.display().to_string())
        .clang_args(args)
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(Wee8Renamer {}))
        .generate()
        .expect("Unable to generate bindings for `v8`!");

    bindings
        .write_to_file(out_path.join("v8_bindings.rs"))
        .expect("Couldn't write bindings");

    let objcopy_names = ["llvm-objcopy", "objcopy", "gobjcopy"];

    let mut objcopy = None;
    for n in objcopy_names {
        if which::which(n).is_ok() {
            objcopy = Some(n);
            break;
        }
    }

    if objcopy.is_none() {
        panic!(
            "No program akin to `objcopy` found\nI searched for these programs in your path: {}",
            objcopy_names.join(", ")
        );
    }

    let objcopy = objcopy.unwrap();

    let syms: Vec<String> = WEE8_RENAMED
        .lock()
        .expect("cannot lock WEE8_RENAMED")
        .iter()
        .map(|(old, new)| {
            // A bit hacky: we need a way to figure out if we're going to target a Mach-O
            // library or an ELF one to take care of the "_" in front of symbols.
            if cfg!(any(target_os = "macos", target_os = "ios")) {
                format!("--redefine-sym=_{old}={new}")
            } else {
                format!("--redefine-sym={old}={new}")
            }
        })
        .collect();
    let prefixed_v8_lib_path = v8_lib_dir.join("libv8prefixed.a");
    let output = std::process::Command::new(objcopy)
        .args(syms)
        .arg(v8_lib_path.display().to_string())
        .arg(prefixed_v8_lib_path.display().to_string())
        .output()
        .expect("objcopy command failed");

    if !output.status.success() {
        panic!(
            "{objcopy} failed with error code {}: {}",
            output.status,
            String::from_utf8(output.stderr).unwrap()
        );
    }

    println!("cargo:rustc-link-lib=static=v8prefixed");
}

#[allow(unused)]
fn main() {
    #[cfg(feature = "v8")]
    build_v8();
}
