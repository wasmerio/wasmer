fn main() {
    println!("cargo:rerun-if-changed=src/napi_bridge_init.cc");
    println!("cargo:rerun-if-changed=src/edge_environment_shim.cc");
    println!("cargo:rerun-if-changed=../v8/src/edge_v8_platform.cc");
    println!("cargo:rerun-if-changed=../v8/src/js_native_api_v8.cc");
    println!("cargo:rerun-if-changed=../v8/src/unofficial_napi.cc");
    println!("cargo:rerun-if-changed=../v8/src/unofficial_napi_error_utils.cc");
    println!("cargo:rerun-if-changed=../v8/src/unofficial_napi_contextify.cc");
    println!("cargo:rerun-if-env-changed=V8_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=V8_LIB_DIR");
    println!("cargo:rerun-if-env-changed=V8_DEFINES");
    println!("cargo:rerun-if-env-changed=NAPI_V8_DEFINES");
    println!("cargo:rerun-if-env-changed=NAPI_V8_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=NAPI_V8_LIBRARY");

    let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    // napi/v8 paths
    let napi_v8_dir = project_root.join("v8");
    let napi_include = project_root.join("include");
    let napi_v8_src = napi_v8_dir.join("src");
    let edge_src = project_root.join("src");
    let libuv_include = if project_root.join("deps/libuv-wasix/include").exists() {
        project_root.join("deps/libuv-wasix/include")
    } else {
        project_root.join("deps/uv/include")
    };

    // V8 paths
    let v8_include = std::env::var("V8_INCLUDE_DIR")
        .or_else(|_| std::env::var("NAPI_V8_INCLUDE_DIR"))
        .expect("V8 include directory not configured; set V8_INCLUDE_DIR or NAPI_V8_INCLUDE_DIR");
    let v8_lib = std::env::var("V8_LIB_DIR").or_else(|_| {
        std::env::var("NAPI_V8_LIBRARY").map(|path| {
            std::path::Path::new(&path)
                .parent()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or(path)
        })
    });
    let v8_lib =
        v8_lib.expect("V8 library directory not configured; set V8_LIB_DIR or NAPI_V8_LIBRARY");

    let v8_include_dir = std::path::Path::new(&v8_include);
    let v8_lib_dir = std::path::Path::new(&v8_lib);
    assert!(
        v8_include_dir.join("v8.h").exists(),
        "V8 headers not found in V8_INCLUDE_DIR={v8_include}"
    );
    assert!(
        v8_lib_dir.join("libv8.a").exists()
            || v8_lib_dir.join("libv8.so").exists()
            || v8_lib_dir.join("libv8.dylib").exists(),
        "V8 library not found in V8_LIB_DIR={v8_lib}"
    );

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
        .include(&v8_include)
        .include(edge_src.to_str().unwrap())
        .include(libuv_include.to_str().unwrap())
        .include(napi_include.to_str().unwrap())
        .include(napi_v8_src.to_str().unwrap())
        .file("src/napi_bridge_init.cc")
        .file("src/edge_environment_shim.cc")
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

    println!("cargo:rustc-link-search=native={v8_lib}");
    let v8_link_kind =
        if v8_lib_dir.join("libv8.so").exists() || v8_lib_dir.join("libv8.dylib").exists() {
            "dylib"
        } else {
            "static"
        };
    println!("cargo:rustc-link-lib={v8_link_kind}=v8");

    let v8_libplatform_kind = if v8_lib_dir.join("libv8_libplatform.a").exists() {
        "static"
    } else {
        "dylib"
    };
    println!("cargo:rustc-link-lib={v8_libplatform_kind}=v8_libplatform");

    let v8_libbase_kind = if v8_lib_dir.join("libv8_libbase.a").exists() {
        "static"
    } else {
        "dylib"
    };
    println!("cargo:rustc-link-lib={v8_libbase_kind}=v8_libbase");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" || target_os == "ios" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=m");
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=rt");
    }
}
