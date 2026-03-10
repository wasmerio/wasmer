#[test]
#[ignore]
fn test_test_vfs() {
    assert_emscripten_output!(
        "../../emtests/test_vfs.wasm",
        "test_vfs",
        vec![],
        "../../emtests/test_vfs.out"
    );
}
