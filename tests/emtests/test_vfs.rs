#[test]
#[ignore]
fn test_test_vfs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_vfs.wasm",
        "test_vfs",
        vec![],
        "../emscripten_resources/emtests/test_vfs.out"
    );
}
