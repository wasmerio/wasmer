#[macro_wasmer_universal_test::universal_test]
fn extern_ref_passed_and_returned() -> Result<(), ()> {
    Ok(())
}

fn main() {
    extern_ref_passed_and_returned_js();
    extern_ref_passed_and_returned().unwrap();
}
