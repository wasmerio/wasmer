#[test]
fn test_execvp() {
    #[cfg(not(target_os = "windows"))]
    assert_emscripten_output!(
        "../../emtests/test_execvp.wasm",
        "test_execvp",
        vec![],
        "../../emtests/test_execvp.out"
    );
    #[cfg(target_os = "windows")]
    assert_emscripten_output!(
        "../../emtests/test_execvp_windows.wasm",
        "test_execvp",
        vec![],
        "../../emtests/test_execvp.out"
    );
}
