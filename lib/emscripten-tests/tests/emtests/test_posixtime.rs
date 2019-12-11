#[test]
#[ignore]
fn test_test_posixtime() {
    assert_emscripten_output!(
        "../../emtests/test_posixtime.wasm",
        "test_posixtime",
        vec![],
        "../../emtests/test_posixtime.out"
    );
}
