#[test]
#[ignore]
fn test_test_uname() {
    assert_emscripten_output!(
        "../../emtests/test_uname.wasm",
        "test_uname",
        vec![],
        "../../emtests/test_uname.out"
    );
}
