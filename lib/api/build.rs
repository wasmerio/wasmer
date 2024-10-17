fn main() {
    #[cfg(feature = "wamr")]
    {
        const WAMR_ZIP: &str = "https://github.com/bytecodealliance/wasm-micro-runtime/archive/refs/tags/WAMR-2.1.0.zip";
        const WAMR_DIR: &str = "wasm-micro-runtime-WAMR-2.1.0";

        use cmake::Config;
        use std::{env, path::PathBuf};

        let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();

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
        let zip_dir = PathBuf::from(&crate_root).join(WAMR_DIR);
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
            .extract(&crate_root)
            .expect("failed to extract wamr zip file");
        let _ = std::fs::remove_dir_all(&wamr_dir);
        std::fs::rename(zip_dir, &wamr_dir).expect("failed to rename wamr dir");

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

        if cfg!(target_os = "windows") {
            dst.define("CMAKE_CXX_COMPILER", "cl.exe");
            dst.define("CMAKE_C_COMPILER", "cl.exe");
            dst.define("CMAKE_LINKER_TYPE", "MSVC");
            dst.define("WAMR_BUILD_PLATFORM", "windows");
            dst.define("WAMR_BUILD_LIBC_UVWASI", "0");
        }

        let dst = dst.build();

        // Check output of `cargo build --verbose`, should see something like:
        // -L native=/path/runng/target/debug/build/runng-sys-abc1234/out
        // That contains output from cmake
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("build").display()
        );
        println!("cargo:rustc-link-lib=vmlib");

        let bindings = bindgen::Builder::default()
            .header(
                wamr_dir
                    .join("core/iwasm/include/wasm_c_api.h")
                    .to_str()
                    .unwrap(),
            )
            .header(
                wamr_dir
                    .join("core/iwasm/include/wasm_export.h")
                    .to_str()
                    .unwrap(),
            )
            .derive_default(true)
            .derive_debug(true)
            .generate()
            .expect("Unable to generate bindings");
        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("bindings.rs"))
            .expect("Couldn't write bindings");
    }

    #[cfg(feature = "wasmi")]
    {
        use std::{env, path::PathBuf};

        let bindings = bindgen::Builder::default()
            .header(
                PathBuf::from(std::env::var("DEP_WASMI_C_API_INCLUDE").unwrap())
                    .join("wasm.h")
                    .to_string_lossy(),
            )
            .derive_default(true)
            .derive_debug(true)
            .generate()
            .expect("Unable to generate bindings for `wasmi`!");
        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
        bindings
            .write_to_file(out_path.join("bindings.rs"))
            .expect("Couldn't write bindings");
    }

    #[cfg(feature = "v8")]
    {
        use std::{env, path::PathBuf};

        let url = match (env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(), env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str()) {
            ("macos", "aarch64") => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.6/wee8-darwin-aarch64.tar.xz",
            ("macos", "x86_64") => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.6/wee8-darwin-amd64.tar.xz",
            ("linux", "x86_64") => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.6/wee8-linux-amd64.tar.xz",
            ("windows", "x86_64") =>"https://github.com/wasmerio/wee8-custom-builds/releases/download/11.6/wee8-windows-amd64.tar.xz",
            (os, arch) => panic!("target os + arch combination not supported: {os}, {arch}")
        };

        let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
        let v8_dir = PathBuf::from(&crate_root).join("third_party").join("v8");
        let out_dir = env::var("OUT_DIR").unwrap();

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

        println!("cargo:rustc-link-lib=static=wee8");
        println!("cargo:rustc-link-lib=v8_initializers");
        println!("cargo:rustc-link-lib=v8_libbase");
        println!("cargo:rustc-link-lib=v8_base_without_compiler");
        println!("cargo:rustc-link-lib=v8_compiler");
        println!("cargo:rustc-link-lib=v8_libplatform");
        println!("cargo:rustc-link-lib=v8_libsampler");
        println!("cargo:rustc-link-lib=v8_snapshot");
        println!("cargo:rustc-link-lib=v8_torque_generated");

        if cfg!(any(target_os = "linux",)) {
            println!("cargo:rustc-link-lib=stdc++");
        } else if cfg!(target_os = "windows") {
            /* do nothing */
            println!("cargo:rustc-link-lib=winmm");
            println!("cargo:rustc-link-lib=dbghelp");
            println!("cargo:rustc-link-lib=shlwapi");
        } else {
            println!("cargo:rustc-link-lib=c++");
        }

        let bindings = bindgen::Builder::default()
            .header(v8_dir.join("wasm.h").to_str().unwrap())
            .derive_default(true)
            .derive_debug(true)
            .generate()
            .expect("Unable to generate bindings for `v8`!");

        let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

        bindings
            .write_to_file(out_path.join("bindings.rs"))
            .expect("Couldn't write bindings");
    }
}
