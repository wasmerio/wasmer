use std::ptr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use wasmer::*;
use wasmer_types::{FastGasCounter, InstanceConfig};

fn get_module_with_start(store: &Store) -> Module {
    let wat = r#"
        (import "host" "func" (func))
        (import "host" "gas" (func (param i32)))
        (memory $mem 1)
        (export "memory" (memory $mem))
        (export "bar" (func $bar))
        (func $foo
            call 0
            i32.const 42
            call 1
            call 0
            i32.const 100
            call 1
            call 0
        )
        (func $bar
            call 0
            i32.const 100
            call 1
        )
        (start $foo)
    "#;

    Module::new(&store, &wat).unwrap()
}

fn get_module(store: &Store) -> Module {
    let wat = r#"
        (import "host" "func" (func))
        (import "host" "has" (func (param i32)))
        (import "host" "gas" (func (param i32)))
        (memory $mem 1)
        (export "memory" (memory $mem))
        (func (export "foo")
            call 0
            i32.const 442
            call 1
            i32.const 42
            call 2
            call 0
            i32.const 100
            call 2
            call 0
        )
        (func (export "bar")
            call 0
            i32.const 100
            call 2
        )
        (func (export "zoo")
            loop
                i32.const 100
                call 2
                br 0
            end
        )
    "#;

    Module::new(&store, &wat).unwrap()
}

fn get_store() -> Store {
    let compiler = Singlepass::default();
    let store = Store::new(&Universal::new(compiler).engine());
    store
}

#[test]
fn test_gas_intrinsic_in_start() {
    let store = get_store();
    let mut gas_counter = FastGasCounter::new(300, 3);
    let module = get_module_with_start(&store);
    static HITS: AtomicUsize = AtomicUsize::new(0);
    let result = Instance::new_with_config(
        &module,
        unsafe { InstanceConfig::new_with_counter(ptr::addr_of_mut!(gas_counter)) },
        &imports! {
            "host" => {
                "func" => Function::new(&store, FunctionType::new(vec![], vec![]), |_values| {
                    HITS.fetch_add(1, SeqCst);
                    Ok(vec![])
                }),
                "gas" => Function::new(&store, FunctionType::new(vec![ValType::I32], vec![]), |_| {
                    // It shall be never called, as call is intrinsified.
                    assert!(false);
                    Ok(vec![])
                }),
            },
        },
    );
    assert!(result.is_err());
    match result {
        Err(InstantiationError::Start(runtime_error)) => {
            assert_eq!(runtime_error.message(), "gas limit exceeded")
        }
        _ => assert!(false),
    }
    // Ensure "func" was called twice.
    assert_eq!(HITS.swap(0, SeqCst), 2);
    // Ensure gas was partially spent.
    assert_eq!(gas_counter.burnt(), 426);
    assert_eq!(gas_counter.gas_limit, 300);
    assert_eq!(gas_counter.opcode_cost, 3);
}

#[test]
fn test_gas_intrinsic_regular() {
    let store = get_store();
    let mut gas_counter = FastGasCounter::new(500, 3);
    let module = get_module(&store);
    static HITS: AtomicUsize = AtomicUsize::new(0);
    let instance = Instance::new_with_config(
        &module,
        unsafe { InstanceConfig::new_with_counter(ptr::addr_of_mut!(gas_counter)) },
        &imports! {
            "host" => {
                "func" => Function::new(&store, FunctionType::new(vec![], vec![]), |_values| {
                    HITS.fetch_add(1, SeqCst);
                    Ok(vec![])
                }),
                "has" => Function::new(&store, FunctionType::new(vec![ValType::I32], vec![]), |_| {
                    HITS.fetch_add(1, SeqCst);
                    Ok(vec![])
                }),
                "gas" => Function::new(&store, FunctionType::new(vec![ValType::I32], vec![]), |_| {
                    // It shall be never called, as call is intrinsified.
                    assert!(false);
                    Ok(vec![])
                }),
            },
        },
    );
    assert!(instance.is_ok());
    let instance = instance.unwrap();
    let foo_func = instance
        .exports
        .get_function("foo")
        .expect("expected function foo");
    let bar_func = instance
        .exports
        .get_function("bar")
        .expect("expected function bar");
    let zoo_func = instance
        .exports
        .get_function("zoo")
        .expect("expected function zoo");
    // Ensure "func" was not called.
    assert_eq!(HITS.load(SeqCst), 0);
    let e = bar_func.call(&[]);
    assert!(e.is_ok());
    // Ensure "func" was called.
    assert_eq!(HITS.load(SeqCst), 1);
    assert_eq!(gas_counter.burnt(), 300);
    let _e = foo_func.call(&[]).err().expect("error calling function");
    // Ensure "func" and "has" was called again.
    assert_eq!(HITS.load(SeqCst), 4);
    assert_eq!(gas_counter.burnt(), 726);
    // Finally try to exhaust rather large limit.
    gas_counter.gas_limit = 10_000_000_000_000_000;
    gas_counter.opcode_cost = 100_000_000;
    let _e = zoo_func.call(&[]).err().expect("error calling function");
    assert_eq!(gas_counter.burnt(), 10_000_000_000_000_726);
}

#[test]
fn test_gas_intrinsic_default() {
    let store = get_store();
    let module = get_module(&store);
    static HITS: AtomicUsize = AtomicUsize::new(0);
    let instance = Instance::new(
        &module,
        &imports! {
            "host" => {
                "func" => Function::new(&store, FunctionType::new(vec![], vec![]), |_values| {
                    HITS.fetch_add(1, SeqCst);
                    Ok(vec![])
                }),
                "has" => Function::new(&store, FunctionType::new(vec![ValType::I32], vec![]), |_| {
                    HITS.fetch_add(1, SeqCst);
                    Ok(vec![])
                }),
                "gas" => Function::new(&store, FunctionType::new(vec![ValType::I32], vec![]), |_| {
                    // It shall be never called, as call is intrinsified.
                    assert!(false);
                    Ok(vec![])
                }),
            },
        },
    );
    assert!(instance.is_ok());
    let instance = instance.unwrap();
    let foo_func = instance
        .exports
        .get_function("foo")
        .expect("expected function foo");
    let bar_func = instance
        .exports
        .get_function("bar")
        .expect("expected function bar");
    // Ensure "func" was called.
    assert_eq!(HITS.load(SeqCst), 0);
    let e = bar_func.call(&[]);
    assert!(e.is_ok());
    // Ensure "func" was called.
    assert_eq!(HITS.load(SeqCst), 1);
    let _e = foo_func.call(&[]);
    // Ensure "func" and "has" was called.
    assert_eq!(HITS.load(SeqCst), 5);
}
