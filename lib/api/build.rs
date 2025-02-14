#[cfg(feature = "wamr")]
fn build_wamr() {
    use bindgen::callbacks::ParseCallbacks;
    const WAMR_ZIP: &str = "https://github.com/bytecodealliance/wasm-micro-runtime/archive/0e4dffc47922bb6fcdcaed7de2a6edfe8c48a7cd.zip";
    const ZIP_NAME: &str = "wasm-micro-runtime-0e4dffc47922bb6fcdcaed7de2a6edfe8c48a7cd";

    use cmake::Config;
    use std::{env, path::PathBuf};

    let crate_root = env::var("OUT_DIR").unwrap();

    // Read target os from cargo env
    // Transform from cargo value to valid wasm-micro-runtime os
    let target_os = match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "darwin",
        "freebsd" => "freebsd",
        "android" => "android",
        "ios" => "ios",
        other => panic!("Unsupported CARGO_CFG_TARGET_OS: {}", other),
    };

    // Read target arch from cargo env
    // Transform from cargo value to valid wasm-micro-runtime WAMR_BUILD_TARGET
    let target_arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86" => "X86_32",
        "x86_64" => "X86_64",
        "arm" => "ARM",
        "aarch64" => "AARCH64",
        "mips" => "MIPS",
        "powerpc" => "POWERPC",
        "powerpc64" => "POWERPC64",
        other => panic!("Unsupported CARGO_CFG_TARGET_ARCH: {}", other),
    };

    // Cleanup tmp data from prior builds
    let wamr_dir = PathBuf::from(&crate_root).join("third_party/wamr");
    let zip_dir = PathBuf::from(&crate_root)
        .join("third_party")
        .join(ZIP_NAME);
    let _ = std::fs::remove_dir_all(&wamr_dir);
    let _ = std::fs::remove_dir_all(&zip_dir);

    // Fetch & extract wasm-micro-runtime source
    let zip = ureq::get(WAMR_ZIP).call().expect("failed to download wamr");
    let mut zip_data = Vec::new();
    zip.into_reader()
        .read_to_end(&mut zip_data)
        .expect("failed to download wamr");
    zip::read::ZipArchive::new(std::io::Cursor::new(zip_data))
        .expect("failed to open wamr zip file")
        .extract(&zip_dir)
        .expect("failed to extract wamr zip file");
    let _ = std::fs::remove_dir_all(&wamr_dir);
    std::fs::rename(zip_dir.join(ZIP_NAME), &wamr_dir).expect("failed to rename wamr dir");

    let wamr_platform_dir = wamr_dir.join("product-mini/platforms").join(target_os);
    let mut dst = Config::new(wamr_platform_dir.as_path());

    dst.always_configure(true)
        .generator("Ninja")
        .no_build_target(true)
        .define(
            "CMAKE_BUILD_TYPE",
            if cfg!(debug_assertions) {
                "RelWithDebInfo"
            } else {
                "Release"
            },
        )
        .define("WAMR_BUILD_AOT", "0")
        //.define("WAMR_BUILD_TAIL_CALL", "1")
        //.define("WAMR_BUILD_DUMP_CALL_STACK", "1")
        // .define("WAMR_BUILD_CUSTOM_NAME_SECTION", "1")
        // .define("WAMR_BUILD_LOAD_CUSTOM_SECTION", "1")
        .define("WAMR_BUILD_BULK_MEMORY", "1")
        .define("WAMR_BUILD_REF_TYPES", "1")
        .define("WAMR_BUILD_SIMD", "1")
        .define("WAMR_BUILD_FAST_INTERP", "1")
        .define("WAMR_BUILD_LIB_PTHREAD", "1")
        .define("WAMR_BUILD_LIB_WASI_THREADS", "0")
        .define("WAMR_BUILD_LIBC_WASI", "0")
        .define("WAMR_BUILD_LIBC_BUILTIN", "0")
        .define("WAMR_BUILD_SHARED_MEMORY", "1")
        .define("WAMR_BUILD_MULTI_MODULE", "0")
        .define("WAMR_DISABLE_HW_BOUND_CHECK", "1")
        .define("WAMR_BUILD_TARGET", target_arch);

    if target_os == "windows" {
        dst.define("CMAKE_CXX_COMPILER", "cl.exe");
        dst.define("CMAKE_C_COMPILER", "cl.exe");
        dst.define("CMAKE_LINKER_TYPE", "MSVC");
        dst.define("WAMR_BUILD_PLATFORM", "windows");
        dst.define("WAMR_BUILD_LIBC_UVWASI", "0");
    }

    //if target_os == "ios" {
    //    // XXX: Hacky
    //    //
    //    // Compiling wamr targeting `aarch64-apple-ios` results in
    //    //
    //    // ```
    //    //  clang: error: unsupported option '-mfloat-abi=' for target 'aarch64-apple-ios'
    //    // ```
    //    // So, here, we simply remove that setting.
    //    //
    //    // See: https://github.com/bytecodealliance/wasm-micro-runtime/pull/3889
    //    let mut lines = vec![];
    //    let cmake_file_path = wamr_platform_dir.join("CMakeLists.txt");
    //    for line in std::fs::read_to_string(&cmake_file_path).unwrap().lines() {
    //        if !line.contains("-mfloat-abi=hard") {
    //            lines.push(line.to_string())
    //        }
    //    }
    //    std::fs::write(cmake_file_path, lines.join("\n")).unwrap();
    //}

    let dst = dst.build();

    // Check output of `cargo build --verbose`, should see something like:
    // -L native=/path/runng/target/debug/build/runng-sys-abc1234/out
    // That contains output from cmake

    // Rename the symbols created from wamr.
    static mut WAMR_RENAMED: Vec<(String, String)> = vec![];

    #[derive(Debug)]
    struct WamrRenamer {}
    impl ParseCallbacks for WamrRenamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = format!("wamr_{}", item_info.name);
                unsafe {
                    WAMR_RENAMED.push((item_info.name.to_string(), new_name.clone()));
                }
                Some(new_name)
            } else {
                None
            }
        }
    }

    let bindings = bindgen::Builder::default()
        .header(
            wamr_dir
                .join("core/iwasm/include/wasm_c_api.h")
                .to_str()
                .unwrap(),
        )
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(WamrRenamer {}))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("wamr_bindings.rs"))
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

    unsafe {
        let syms: Vec<String> = WAMR_RENAMED
            .iter()
            .map(|(old, new)|
                // A bit hacky: we need a way to figure out if we're going to target a Mach-O
                // library or an ELF one to take care of the "_" in front of symbols.
            {
                if cfg!(any(target_os = "macos", target_os = "ios")) {
                    format!("--redefine-sym=_{old}={new}")
                } else {
                    format!("--redefine-sym={old}={new}")
                }
            })
            .collect();
        let output = std::process::Command::new(objcopy)
            .args(syms)
            .arg(dst.join("build").join("libvmlib.a").display().to_string())
            .arg(dst.join("build").join("libwamr.a").display().to_string())
            .output()
            .unwrap();

        if !output.status.success() {
            panic!(
                "{objcopy} failed with error code {}: {}",
                output.status,
                String::from_utf8(output.stderr).unwrap()
            );
        }
    }

    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("build").display()
    );
    println!("cargo:rustc-link-lib=wamr");
}

#[cfg(feature = "v8")]
fn build_v8() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{env, path::PathBuf};

    let url = match (
            env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(),
            env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str(),
            env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default().as_str(),
        ) {
            ("macos", "aarch64", _) => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-darwin-aarch64.tar.xz",
            ("macos", "x86_64", _) => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-darwin-amd64.tar.xz",
            ("linux", "x86_64", "gnu") => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-linux-amd64.tar.xz",
            ("linux", "x86_64", "musl") => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-linux-musl-amd64.tar.xz",
            // Not supported in 6.0.0-alpha1
            //("windows", "x86_64", _) => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-windows-amd64.tar.xz",
            (os, arch, _) => panic!("target os + arch combination not supported: {os}, {arch}"),
        };

    let out_dir = env::var("OUT_DIR").unwrap();
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let v8_header_path = PathBuf::from(&crate_root).join("third-party").join("wee8");

    let tar = ureq::get(url).call().expect("failed to download v8");

    let mut tar_data = Vec::new();
    tar.into_reader()
        .read_to_end(&mut tar_data)
        .expect("failed to download v8 lib");

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().unwrap() {
        eprintln!("entry: {:?}", entry.unwrap().path());
    }

    let tar = xz::read::XzDecoder::new(tar_data.as_slice());
    let mut archive = tar::Archive::new(tar);

    archive.unpack(out_dir.clone()).unwrap();
    println!("cargo:rustc-link-search=native={}", out_dir);

    if cfg!(any(target_os = "linux",)) {
        println!("cargo:rustc-link-lib=stdc++");
    } else if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=dbghelp");
        println!("cargo:rustc-link-lib=shlwapi");
    } else {
        println!("cargo:rustc-link-lib=c++");
    }

    // Rename the symbols created from wee8.
    static mut WEE8_RENAMED: Vec<(String, String)> = vec![];

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
                unsafe {
                    WEE8_RENAMED.push((item_info.name.to_string(), new_name.clone()));
                }
                Some(new_name)
            } else {
                None
            }
        }
    }

    let header_path = v8_header_path.join("wasm.h");
    let bindings = bindgen::Builder::default()
        .header(header_path.display().to_string())
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(Wee8Renamer {}))
        .generate()
        .expect("Unable to generate bindings for `v8`!");

    let out_path = PathBuf::from(out_dir);

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

    unsafe {
        let syms: Vec<String> = WEE8_RENAMED
            .iter()
            .map(|(old, new)|
                // A bit hacky: we need a way to figure out if we're going to target a Mach-O
                // library or an ELF one to take care of the "_" in front of symbols.
            {
                if cfg!(any(target_os = "macos", target_os = "ios")) {
                    format!("--redefine-sym=_{old}={new}")
                } else {
                    format!("--redefine-sym={old}={new}")
                }
            })
            .collect();
        let output = dbg!(std::process::Command::new(objcopy)
            .args(syms)
            .arg(out_path.join("obj").join("libwee8.a").display().to_string())
            .arg(out_path.join("libwee8prefixed.a").display().to_string()))
        .output()
        .unwrap();

        if !output.status.success() {
            panic!(
                "{objcopy} failed with error code {}: {}",
                output.status,
                String::from_utf8(output.stderr).unwrap()
            );
        }
    }

    println!("cargo:rustc-link-lib=static=wee8prefixed");
}

#[cfg(feature = "wasmi")]
fn build_wasmi() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{env, path::PathBuf};

    #[derive(Debug)]
    struct WasmiRenamer {}

    impl ParseCallbacks for WasmiRenamer {
        /// This function will run for every extern variable and function. The returned value determines
        /// the link name in the bindings.
        fn generated_link_name_override(
            &self,
            item_info: bindgen::callbacks::ItemInfo<'_>,
        ) -> Option<String> {
            if item_info.name.starts_with("wasm") {
                let new_name = if cfg!(any(target_os = "macos", target_os = "ios")) {
                    format!("_wasmi_{}", item_info.name)
                } else {
                    format!("wasmi_{}", item_info.name)
                };

                Some(new_name)
            } else {
                None
            }
        }
    }

    let bindings = bindgen::Builder::default()
        .header(
            PathBuf::from(std::env::var("DEP_WASMI_C_API_INCLUDE").unwrap())
                .join("wasm.h")
                .to_string_lossy(),
        )
        .derive_default(true)
        .derive_debug(true)
        .parse_callbacks(Box::new(WasmiRenamer {}))
        .generate()
        .expect("Unable to generate bindings for `wasmi`!");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("wasmi_bindings.rs"))
        .expect("Couldn't write bindings");
}
#[allow(unused)]
fn main() {
    #[cfg(feature = "wamr")]
    build_wamr();

    #[cfg(feature = "v8")]
    build_v8();

    #[cfg(feature = "wasmi")]
    build_wasmi();
}
