use cbindgen::{Builder, Language};
use std::{env, fs, path::PathBuf};

const PRE_HEADER: &'static str = r#"
// Define the `ARCH_X86_X64` constant.
#if defined(MSVC) && defined(_M_AMD64)
#  define ARCH_X86_64
#elif (defined(GCC) || defined(__GNUC__) || defined(__clang__)) && defined(__x86_64__)
#  define ARCH_X86_64
#endif

// Compatibility with non-Clang compilers.
#if !defined(__has_attribute)
#  define __has_attribute(x) 0
#endif

// Compatibility with non-Clang compilers.
#if !defined(__has_declspec_attribute)
#  define __has_declspec_attribute(x) 0
#endif

// Define the `DEPRECATED` macro.
#if defined(GCC) || defined(__GNUC__) || __has_attribute(deprecated)
#  define DEPRECATED(message) __attribute__((deprecated(message)))
#elif defined(MSVC) || __has_declspec_attribute(deprecated)
#  define DEPRECATED(message) __declspec(deprecated(message))
#endif
"#;

const WASI_FEATURE_AS_C_DEFINE: &'static str = "WASMER_WASI_ENABLED";
const EMSCRIPTEN_FEATURE_AS_C_DEFINE: &'static str = "WASMER_EMSCRIPTEN_ENABLED";

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    build_wasmer_headers(&crate_dir, &out_dir);
}

fn build_wasmer_headers(crate_dir: &str, out_dir: &str) {
    let mut crate_header_file = PathBuf::from(crate_dir);
    crate_header_file.push("wasmer");

    let mut out_header_file = PathBuf::from(out_dir);
    out_header_file.push("wasmer");

    let mut pre_header = format!(
        r#"// The Wasmer C/C++ header file.

#if !defined(WASMER_H_MACROS)

#define WASMER_H_MACROS
{pre_header}"#,
        pre_header = PRE_HEADER
    );

    #[cfg(feature = "wasi")]
    {
        pre_header.push_str(&format!(
            r#"
// The `wasi` feature has been enabled for this build.
#define {define}

"#,
            define = WASI_FEATURE_AS_C_DEFINE
        ));
    }

    #[cfg(feature = "emscripten")]
    {
        pre_header.push_str(&format!(
            r#"
// The `emscripten` feature has been enabled for this build.
#define {define}

"#,
            define = EMSCRIPTEN_FEATURE_AS_C_DEFINE
        ));
    }

    // Close pre header.
    pre_header.push_str(
        r#"#endif // WASMER_H_MACROS


//
// OK, here we go. The code below is automatically generated.
//
"#,
    );

    // C bindings.
    {
        // Generate the bindings in the `OUT_DIR`.
        out_header_file.set_extension("h");

        Builder::new()
            .with_language(Language::C)
            .with_crate(crate_dir)
            .with_include_guard("WASMER_H")
            .with_header(&pre_header)
            .with_define("target_family", "windows", "_WIN32")
            .with_define("target_arch", "x86_64", "ARCH_X86_64")
            .with_define("feature", "wasi", WASI_FEATURE_AS_C_DEFINE)
            .with_define("feature", "emscripten", EMSCRIPTEN_FEATURE_AS_C_DEFINE)
            .with_documentation(true)
            .generate()
            .expect("Unable to generate C bindings")
            .write_to_file(out_header_file.as_path());

        // Copy the generated bindings from `OUT_DIR` to
        // `CARGO_MANIFEST_DIR`.
        crate_header_file.set_extension("h");

        fs::copy(out_header_file.as_path(), crate_header_file.as_path())
            .expect("Unable to copy the generated C bindings");
    }

    // C++ bindings.
    {
        // Generate the bindings in the `OUT_DIR`.
        out_header_file.set_extension("hh");

        Builder::new()
            .with_language(Language::Cxx)
            .with_crate(crate_dir)
            .with_include_guard("WASMER_H")
            .with_header(&pre_header)
            .with_define("target_family", "windows", "_WIN32")
            .with_define("target_arch", "x86_64", "ARCH_X86_64")
            .with_define("feature", "wasi", WASI_FEATURE_AS_C_DEFINE)
            .with_define("feature", "emscripten", EMSCRIPTEN_FEATURE_AS_C_DEFINE)
            .with_documentation(true)
            .generate()
            .expect("Unable to generate C++ bindings")
            .write_to_file(out_header_file.as_path());

        // Copy the generated bindings from `OUT_DIR` to
        // `CARGO_MANIFEST_DIR`.
        crate_header_file.set_extension("hh");

        fs::copy(out_header_file, crate_header_file)
            .expect("Unable to copy the generated C++ bindings");
    }
}
