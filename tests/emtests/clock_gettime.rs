#[test]
fn test_clock_gettime() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/clock_gettime.wasm",
        "clock_gettime",
        vec![],
        "../emscripten_resources/emtests/clock_gettime.out"
    );
}
