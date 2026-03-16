use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn download_v8() {
    let url = match (
        env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ENV")
            .unwrap_or_default()
            .as_str(),
    ) {
        ("macos", "aarch64", _) => {
            "https://github.com/wasmerio/v8-custom-builds/releases/download/11.9.2/v8-darwin-arm64.tar.xz"
        }
        ("macos", "x86_64", _) => {
            "https://github.com/wasmerio/v8-custom-builds/releases/download/11.9.2/v8-darwin-amd64.tar.xz"
        }
        ("linux", "x86_64", "gnu") => {
            "https://github.com/wasmerio/v8-custom-builds/releases/download/11.9.2/v8-linux-amd64.tar.xz"
        }
        ("linux", "x86_64", "musl") => {
            "https://github.com/wasmerio/v8-custom-builds/releases/download/11.9.2/v8-linux-musl-amd64.tar.xz"
        }
        ("android", "aarch64", _) => {
            "https://github.com/wasmerio/v8-custom-builds/releases/download/11.9.2/v8-android-arm64.tar.xz"
        }
        (os, arch, _) => panic!("target os + arch combination not supported: {os}, {arch}"),
    };

    let out_dir = env::var("OUT_DIR").unwrap();

    let tar_data = ureq::get(url)
        .call()
        .expect("failed to download v8")
        .body_mut()
        .with_config()
        .limit(50 * 1024 * 1024) // 50MB
        .read_to_vec()
        .expect("failed to download v8 lib");

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().unwrap() {
        eprintln!("entry: {:?}", entry.unwrap().path());
    }

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    archive.unpack(out_dir.clone()).unwrap();
}

fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn macos_sdk_path() -> Option<String> {
    if let Ok(sdkroot) = env::var("SDKROOT") {
        let sdkroot = sdkroot.trim();
        if !sdkroot.is_empty() {
            return Some(sdkroot.to_string());
        }
    }

    command_output("xcrun", &["--sdk", "macosx", "--show-sdk-path"])
}

fn clang_resource_dir() -> Option<String> {
    if let Ok(cxx) = env::var("CXX") {
        let cxx = cxx.trim();
        if !cxx.is_empty() {
            if let Some(path) = command_output(cxx, &["-print-resource-dir"]) {
                return Some(path);
            }
        }
    }

    command_output("clang++", &["-print-resource-dir"])
}

fn main() {
    println!("cargo:rerun-if-changed=src/napi_bridge_init.cc");
    println!("cargo:rerun-if-changed=v8/src/edge_v8_platform.cc");
    println!("cargo:rerun-if-changed=v8/src/js_native_api_v8.cc");
    println!("cargo:rerun-if-changed=v8/src/unofficial_napi.cc");
    println!("cargo:rerun-if-changed=v8/src/unofficial_napi_error_utils.cc");
    println!("cargo:rerun-if-changed=v8/src/unofficial_napi_contextify.cc");
    println!("cargo:rerun-if-env-changed=V8_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=V8_LIB_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    println!("cargo:rerun-if-env-changed=CXX");

    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // napi/v8 paths
    let napi_v8_dir = project_root.join("v8");
    let napi_include = project_root.join("include");
    let napi_v8_src = napi_v8_dir.join("src");
    let edge_src = project_root.join("src");

    // V8 paths
    let v8_include = std::env::var("V8_INCLUDE_DIR");
    let v8_lib = std::env::var("V8_LIB_DIR");

    let (v8_include_dir, v8_lib_dir): (PathBuf, PathBuf) =
        if let (Ok(v8_include), Ok(v8_lib)) = (&v8_include, &v8_lib) {
            (PathBuf::from(v8_include), PathBuf::from(v8_lib))
        } else {
            download_v8();
            let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
            (out_dir.join("include"), out_dir.join("lib"))
        };

    assert!(
        v8_include_dir.join("v8.h").exists(),
        "V8 headers not found in V8_INCLUDE_DIR={v8_include:?}"
    );
    assert!(
        v8_lib_dir.join("libv8.a").exists()
            || v8_lib_dir.join("libv8.so").exists()
            || v8_lib_dir.join("libv8.dylib").exists(),
        "V8 library not found in V8_LIB_DIR={v8_lib:?}"
    );
    println!("cargo:rustc-link-search=native={}", v8_lib_dir.display());

    let v8_defines = std::env::var("V8_DEFINES")
        .or_else(|_| std::env::var("NAPI_V8_DEFINES"))
        .unwrap_or_else(|_| "V8_COMPRESS_POINTERS".to_string());

    // Compile the napi/v8 sources + bridge into a single static library.
    // Keep V8 feature defines aligned with the selected V8 binary.
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .flag_if_supported("-std=c++20")
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("-w")
        .define("NAPI_EXTERN", Some(""))
        .include(&v8_include_dir)
        .include(edge_src.to_str().unwrap())
        .include(napi_include.to_str().unwrap())
        .include(napi_v8_src.to_str().unwrap())
        .file("src/napi_bridge_init.cc")
        .file(napi_v8_src.join("js_native_api_v8.cc").to_str().unwrap())
        .file(napi_v8_src.join("unofficial_napi.cc").to_str().unwrap())
        .file(
            napi_v8_src
                .join("unofficial_napi_error_utils.cc")
                .to_str()
                .unwrap(),
        )
        .file(
            napi_v8_src
                .join("unofficial_napi_contextify.cc")
                .to_str()
                .unwrap(),
        )
        .file(napi_v8_src.join("edge_v8_platform.cc").to_str().unwrap());

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        build.flag_if_supported("-stdlib=libc++");

        if let Some(sdk_path) = macos_sdk_path() {
            build.flag("-isysroot");
            build.flag(&sdk_path);
            build.flag("-nostdinc++");
            build.flag("-isystem");
            build.flag(&format!("{sdk_path}/usr/include/c++/v1"));
        }

        if let Some(resource_dir) = clang_resource_dir() {
            build.flag("-isystem");
            build.flag(&format!("{resource_dir}/include"));
        }
    }

    for raw in v8_defines.split(&[';', ',', ' '][..]) {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((name, value)) = entry.split_once('=') {
            build.define(name.trim(), Some(value.trim()));
        } else {
            build.define(entry, Some("1"));
        }
    }

    build.compile("napi_bridge");

    let v8_link_kind =
        if v8_lib_dir.join("libv8.so").exists() || v8_lib_dir.join("libv8.dylib").exists() {
            "dylib"
        } else {
            "static"
        };
    println!("cargo:rustc-link-lib={v8_link_kind}=v8");

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=m");
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=rt");
    }
}
