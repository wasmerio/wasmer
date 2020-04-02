#[test]
#[ignore]
fn test_test_posixtime() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_posixtime.wasm",
        "test_posixtime",
        vec![],
        "../emscripten_resources/emtests/test_posixtime.out"
    );
}
