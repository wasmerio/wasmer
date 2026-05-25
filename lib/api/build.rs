#[cfg(feature = "v8")]
fn build_v8() {
    use bindgen::callbacks::ParseCallbacks;
    use std::{
        env, fs,
        path::PathBuf,
        sync::{LazyLock, Mutex},
    };

    const LOCAL_V8_LIB_PATH: &str =
        "/home/marxin/Programming/wee8-custom-builds/v8/out/debug/obj/libwee8.a";

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = PathBuf::from(&out_dir);
    let crate_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let v8_header_path = PathBuf::from(&crate_root).join("third-party").join("wee8");
    let v8_lib_path = PathBuf::from(LOCAL_V8_LIB_PATH);
    let v8_lib_dir = out_path.join("wee8-lib");
    fs::create_dir_all(&v8_lib_dir).unwrap_or_else(|err| {
        panic!(
            "failed to create prefixed v8 lib dir {}: {err}",
            v8_lib_dir.display()
        )
    });
    if !v8_lib_path.exists() {
        panic!("local v8 lib does not exist: {}", v8_lib_path.display());
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
