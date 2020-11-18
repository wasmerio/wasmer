mod test_c_helpers;

use test_c_helpers::compile_with_cmake_and_run_test;

#[test]
#[cfg(feature = "deprecated")]
fn test_deprecated_c_api() {
    let project_tests_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/deprecated/");

    let cmake_args = vec![
        ".",
        #[cfg(feature = "wasi")]
        "-DWASI_TESTS=ON",
        #[cfg(feature = "emscripten")]
        "-DEMSCRIPTEN_TESTS=ON",
        // We need something like this to get this working on Windows, but this doesn't seem
        // quite right -- perhaps it's double escaping the quotes?
        #[cfg(target_os = "windows")]
        r#"-G "MinGW Makefiles""#,
    ];

    compile_with_cmake_and_run_test(project_tests_dir, cmake_args);
}
