#[cfg(feature = "v8")]
fn build_v8() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{
        env,
        path::PathBuf,
        sync::{LazyLock, Mutex},
    };

    let url = match (
        env::var("CARGO_CFG_TARGET_OS").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str(),
        env::var("CARGO_CFG_TARGET_ENV")
            .unwrap_or_default()
            .as_str(),
    ) {
        ("macos", "aarch64", _) => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-darwin-aarch64.tar.xz"
        }
        ("linux", "x86_64", "gnu") => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-linux-amd64.tar.xz"
        }
        ("linux", "x86_64", "musl") => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-linux-musl-amd64.tar.xz"
        }
        ("android", "aarch64", _) => {
            "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.8/wee8-android-arm64.tar.xz"
        }
        // Not supported in 6.0.0-alpha1
        //("windows", "x86_64", _) => "https://github.com/wasmerio/wee8-custom-builds/releases/download/11.7-custom1/wee8-windows-amd64.tar.xz",
        (os, arch, _) => panic!("target os + arch combination not supported: {os}, {arch}"),
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let v8_header_path = PathBuf::from(&crate_root).join("third-party").join("wee8");

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
    println!("cargo:rustc-link-search=native={out_dir}");

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

    let syms: Vec<String> = WEE8_RENAMED.lock()
                        .expect("cannot lock WEE8_RENAMED")
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
    let output = dbg!(
        std::process::Command::new(objcopy)
            .args(syms)
            .arg(out_path.join("obj").join("libwee8.a").display().to_string())
            .arg(out_path.join("libwee8prefixed.a").display().to_string())
    )
    .output()
    .unwrap();

    if !output.status.success() {
        panic!(
            "{objcopy} failed with error code {}: {}",
            output.status,
            String::from_utf8(output.stderr).unwrap()
        );
    }

    println!("cargo:rustc-link-lib=static=wee8prefixed");
}

#[allow(unused)]
fn main() {
    #[cfg(feature = "v8")]
    build_v8();
}
