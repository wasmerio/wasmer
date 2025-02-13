#[test]
#[cfg(all(feature = "sys", feature = "wamr", feature = "v8"))]
fn can_create_multiple_engines() {
    use wasmer::{sys::Cranelift, v8::V8, wamr::Wamr, *};
    let _: Engine = Cranelift::new().into();

    #[cfg(feature = "v8")]
    {
        let _: Engine = V8::new().into();
    }

    #[cfg(feature = "wamr")]
    {
        let _: Engine = Wamr::new().into();
    }
}

#[test]
#[cfg(all(feature = "v8", feature = "sys"))]
fn multiple_engines_can_run_together() {
    use std::u8;
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

#[test]
#[cfg(all(feature = "sys", feature = "wamr", feature = "v8"))]
fn engine_unique_id() {
    use std::collections::HashSet;

    use wasmer::{sys::Cranelift, v8::V8, wamr::Wamr, *};

    let mut table = HashSet::new();

    for _ in 0..100_000 {
        let e: Engine = Cranelift::new().into();

        let id = e.id();

        assert!(!table.contains(&id));
        table.insert(e.id());
        assert!(table.contains(&id));

        let e: Engine = V8::new().into();
        let id = e.id();

        assert!(!table.contains(&id));
        table.insert(e.id());
        assert!(table.contains(&id));

        let e: Engine = Wamr::new().into();
        let id = e.id();

        assert!(!table.contains(&id));
        table.insert(e.id());
        assert!(table.contains(&id));
    }
}
