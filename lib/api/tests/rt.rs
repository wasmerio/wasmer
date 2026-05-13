#[test]
#[cfg(all(feature = "v8", feature = "sys"))]
fn multiple_engines_can_run_together() {
    use wasmer::{sys::Cranelift, v8::V8, *};

    let clift: Engine = Cranelift::new().into();
    let mut clift_store = Store::new(clift);
    let c_hello = Function::new_typed(&mut clift_store, move || {
        println!("hello from cranelift!");
    });

    #[cfg(feature = "v8")]
    {
        let v8: Engine = V8::new().into();
        let mut v8_store = Store::new(v8);
        let v8_hello = Function::new_typed(&mut v8_store, move || {
            println!("hello from v8!");
        });
        c_hello.call(&mut clift_store, &[]).unwrap();
        v8_hello.call(&mut v8_store, &[]).unwrap();
    }
}
