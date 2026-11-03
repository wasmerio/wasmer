use macro_wasmer_engine_test::engine_test;
use wasmer::*;

#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

#[engine_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn typed_host_function_closure_panics() -> Result<(), String> {
    let mut store = Store::default();
    let state = 3;

    Function::new_typed(&mut store, move |_: i32| {
        println!("{state}");
    });

    Ok(())
}

#[engine_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn typed_with_env_host_function_closure_panics() -> Result<(), String> {
    let mut store = Store::default();
    let env: i32 = 4;
    let env = FunctionEnv::new(&mut store, env);
    let state = 3;
    Function::new_typed_with_env(
        &mut store,
        &env,
        move |_env: FunctionEnvMut<i32>, _: i32| {
            println!("{state}");
        },
    );

    Ok(())
}

#[engine_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn non_typed_functions_and_closures_with_no_env_work() -> anyhow::Result<()> {
    let mut store = Store::default();
    let wat = r#"(module
        (func $multiply1 (import "env" "multiply1") (param i32 i32) (result i32))
        (func $multiply2 (import "env" "multiply2") (param i32 i32) (result i32))
        (func $multiply3 (import "env" "multiply3") (param i32 i32) (result i32))
        (func $multiply4 (import "env" "multiply4") (param i32 i32) (result i32))

        (func (export "test") (param i32 i32 i32 i32 i32) (result i32)
           (call $multiply4
             (call $multiply3
               (call $multiply2
                  (call $multiply1
                    (local.get 0)
                    (local.get 1))
                  (local.get 2))
               (local.get 3))
              (local.get 4)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let env: i32 = 10;
    let env = FunctionEnv::new(&mut store, env);
    let ty = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    let captured_by_closure = 20;
    let import_object = imports! {
        "env" => {
            "multiply1" => Function::new_with_env(&mut store, &env, &ty, move |_env, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure)])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply2" => Function::new_with_env(&mut store, &env, &ty, move |env, args| {
                if let (Value::I32(v1), Value::I32(v2)) = (&args[0], &args[1]) {
                    Ok(vec![Value::I32(v1 * v2 * captured_by_closure * env.data())])
                } else {
                    panic!("Invalid arguments");
                }
            }),
            "multiply3" => Function::new_typed_with_env(&mut store, &env, |_env: FunctionEnvMut<_>, arg1: i32, arg2: i32| -> i32
                                                {arg1 * arg2 }),
            "multiply4" => Function::new_typed_with_env(&mut store, &env, |env: FunctionEnvMut<i32>, arg1: i32, arg2: i32| -> i32
                                                         {arg1 * arg2 * env.data() }),
        },
    };

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let test: TypedFunction<(i32, i32, i32, i32, i32), i32> =
        instance.exports.get_typed_function(&store, "test")?;

    let result = test.call(&mut store, 2, 3, 4, 5, 6)?;
    let manually_computed_result = 6 * (5 * (4 * (3 * 2 * 20) * 10 * 20)) * 10;
    assert_eq!(result, manually_computed_result);
    Ok(())
}

static STATIC_CONTEXT_VAL: i32 = 1234;

#[engine_test]
#[cfg_attr(
    feature = "js",
    ignore = "Closures with context are not supported in JS yet"
)]
fn holochain_typed_function() -> anyhow::Result<()> {
    // Declare WASM Module
    let wasm_bytes = wat2wasm(
        br#"
(module
  (func $multiply_typed 
      (import "env" "multiply_typed") 
      (param i32) 
      (result i32)
  )
  (type $sum_t 
    (func (param i32) (param i32) (result i32))
  )
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    (call $multiply_typed 
      (local.get $y)
    )
  )
(export "sum" (func $sum_f)))
"#,
    )?;
    let mut store = Store::default();
    struct MyEnv {}
    let env = FunctionEnv::new(&mut store, MyEnv {});
    let module = Module::new(&store, wasm_bytes)?;

    // Define some context data that the host function closure will use
    static STATIC_CONTEXT_VAL2: i32 = 1234;
    let context_val = 1234;
    fn my_val() -> i32 {
        1234
    }

    // Define the host function closure
    let multiply_by_3 = move |_env: FunctionEnvMut<MyEnv>, a: i32| -> i32 {
        assert_eq!(STATIC_CONTEXT_VAL, 1234);
        assert_eq!(STATIC_CONTEXT_VAL2, 1234);
        assert_eq!(context_val, 1234);
        assert_eq!(my_val(), 1234);

        a * 3
    };

    // Define the host function and WASM instance
    let multiply_typed = Function::new_typed_with_env(&mut store, &env, multiply_by_3);
    let import_object = imports! {
        "env" => {
            "multiply_typed" => multiply_typed,
        }
    };
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Execute the WASM function 'sum'
    let sum: TypedFunction<(i32, i32), i32> =
        instance.exports.get_function("sum")?.typed(&store)?;
    let result = sum.call(&mut store, 1, 2)?;
    assert_eq!(result, 6);

    Ok(())
}

// Match the sys-only storage mutation proofs in function::env::split_ownership.
// Every negative discriminator runs before freeing a target that may be active.
#[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
mod target_ownership {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use wasmer::sys::vm::{StoreObject, VMFunction, VMInstance};

    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn active_host_targets_survive_replacement_and_stale_typed_handles_reject() {
        for dynamic in [false, true] {
            for typed in [false, true] {
                let mut store = Store::default();
                let drops = Arc::new(AtomicUsize::new(0));
                let probe = Capture(drops.clone());
                let index = VMFunction::list(store.objects_mut().as_sys_mut()).len();
                let env = FunctionEnv::new(&mut store, index);
                let body = move |mut current: FunctionEnvMut<usize>| -> i32 {
                    std::hint::black_box(&probe);
                    let index = *current.data();
                    let mut actual = current.as_store_mut();
                    // This assertion makes the missing-pin mutation fail while
                    // the Store still owns F. Never free a live &F in a RED run.
                    assert!(
                        VMFunction::list(actual.objects_mut().as_sys_mut())[index].owner_count()
                            > 1,
                        "active host target has no independent snapshot owner"
                    );
                    let _dummy = Function::new_typed(&mut actual, |_: i64| -> i64 { 9 });
                    let functions = VMFunction::list_mut(actual.objects_mut().as_sys_mut());
                    let replacement = functions.pop().unwrap();
                    drop(std::mem::replace(&mut functions[index], replacement));
                    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
                    42
                };
                let function = if dynamic {
                    Function::new_with_env(
                        &mut store,
                        &env,
                        FunctionType::new([], [Type::I32]),
                        move |env, _| Ok(vec![Value::I32(body(env))]),
                    )
                } else {
                    Function::new_typed_with_env(&mut store, &env, body)
                };
                let typed_function = function.typed::<(), i32>(&store).unwrap();
                if typed {
                    assert_eq!(typed_function.call(&mut store).unwrap(), 42);
                } else {
                    assert_eq!(&*function.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
                }
                assert_eq!(drops.load(Ordering::SeqCst), 1);
                // Check metadata first: omitting identity validation must fail
                // here, before an old typed ABI could invoke the i64 replacement.
                assert!(
                    catch_unwind(AssertUnwindSafe(|| function.ty(&store))).is_err(),
                    "a reused function slot changed an existing handle's target"
                );
                assert!(
                    catch_unwind(AssertUnwindSafe(|| typed_function.call(&mut store))).is_err()
                );
            }
        }
    }

    #[test]
    fn wasm_imports_keep_their_original_host_target_after_slot_replacement() {
        for dynamic in [false, true] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Capture(drops.clone());
            let index = VMFunction::list(store.objects_mut().as_sys_mut()).len();
            let body = move || -> i32 {
                std::hint::black_box(&probe);
                42
            };
            let function = if dynamic {
                Function::new(&mut store, FunctionType::new([], [Type::I32]), move |_| {
                    Ok(vec![Value::I32(body())])
                })
            } else {
                Function::new_typed(&mut store, body)
            };
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func $host (result i32)))
                (func (export "run") (result i32) call $host))"#,
            )
            .unwrap();
            let imports = imports! { "env" => { "host" => function } };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            assert!(
                VMFunction::list(store.objects_mut().as_sys_mut())[index].owner_count() > 1,
                "an initialized instance has no owner for its imported target"
            );
            let _dummy = Function::new_typed(&mut store, || -> i32 { 9 });
            let functions = VMFunction::list_mut(store.objects_mut().as_sys_mut());
            let replacement = functions.pop().unwrap();
            drop(std::mem::replace(&mut functions[index], replacement));
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            let run = instance
                .exports
                .get_typed_function::<(), i32>(&store, "run")
                .unwrap();
            assert_eq!(run.call(&mut store).unwrap(), 42);
            assert_eq!(run.call(&mut store).unwrap(), 42);
            // Retire local export descriptors before their Instance owner; the
            // descriptors now independently retain that allocation.
            VMFunction::list_mut(store.objects_mut().as_sys_mut()).clear();
            let retired = VMInstance::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            drop(retired);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn function_descriptors_cannot_be_rebound_by_copying_the_store_id() {
        let mut store = Store::default();
        let original = Function::new_typed(&mut store, || -> i32 { 42 });
        fn send_sync<T: Send + Sync>(_: &T) {}
        send_sync(original.as_sys());
        let descriptor = VMFunction::list(store.objects_mut().as_sys_mut())
            .last()
            .unwrap()
            .clone();
        let original_id = store.id();
        let mut other = Store::default();
        other.objects_mut().as_sys_mut().set_id(original_id);
        let handle =
            wasmer::sys::vm::StoreHandle::new(other.objects_mut().as_sys_mut(), descriptor);
        let raw = wasmer::sys::vm::VMExtern::Function(handle.internal_handle());
        assert!(
            catch_unwind(AssertUnwindSafe(|| wasmer::VMExternToExtern::to_extern(
                raw, &mut other
            )))
            .is_err()
        );
        assert_eq!(&*original.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
    }

    // Follow the existing TinyTunables forwarding sibling in sys/tunables.rs.
    struct ReplacingTunables {
        base: wasmer::sys::BaseTunables,
        original: usize,
        replacement: usize,
        calls: Arc<AtomicUsize>,
    }
    impl wasmer::sys::Tunables for ReplacingTunables {
        fn memory_style(&self, ty: &MemoryType) -> wasmer::sys::vm::MemoryStyle {
            self.base.memory_style(ty)
        }
        fn table_style(&self, ty: &TableType) -> wasmer::sys::vm::TableStyle {
            self.base.table_style(ty)
        }
        fn create_host_memory(
            &self,
            ty: &MemoryType,
            style: &wasmer::sys::vm::MemoryStyle,
        ) -> Result<wasmer::sys::vm::VMMemory, wasmer::sys::vm::MemoryError> {
            self.base.create_host_memory(ty, style)
        }
        unsafe fn create_vm_memory(
            &self,
            ty: &MemoryType,
            style: &wasmer::sys::vm::MemoryStyle,
            location: wasmer::sys::vm::VMDefinitionLocation<wasmer::sys::vm::VMMemoryDefinition>,
        ) -> Result<wasmer::sys::vm::VMMemory, wasmer::sys::vm::MemoryError> {
            unsafe { self.base.create_vm_memory(ty, style, location) }
        }
        fn create_host_table(
            &self,
            ty: &TableType,
            style: &wasmer::sys::vm::TableStyle,
        ) -> Result<wasmer::sys::vm::VMTable, String> {
            self.base.create_host_table(ty, style)
        }
        unsafe fn create_vm_table(
            &self,
            ty: &TableType,
            style: &wasmer::sys::vm::TableStyle,
            location: wasmer::sys::vm::VMDefinitionLocation<wasmer::sys::vm::VMTableDefinition>,
        ) -> Result<wasmer::sys::vm::VMTable, String> {
            unsafe { self.base.create_vm_table(ty, style, location) }
        }
        fn vmconfig(&self) -> &wasmer::sys::vm::VMConfig {
            self.base.vmconfig()
        }
        unsafe fn create_memories(
            &self,
            context: &mut wasmer::sys::vm::StoreObjects,
            module: &wasmer_types::ModuleInfo,
            styles: &wasmer_types::entity::PrimaryMap<
                wasmer_types::MemoryIndex,
                wasmer::sys::vm::MemoryStyle,
            >,
            locations: &[wasmer::sys::vm::VMDefinitionLocation<
                wasmer::sys::vm::VMMemoryDefinition,
            >],
        ) -> Result<
            wasmer_types::entity::PrimaryMap<
                wasmer_types::LocalMemoryIndex,
                wasmer::sys::vm::InternalStoreHandle<wasmer::sys::vm::VMMemory>,
            >,
            wasmer_compiler::LinkError,
        > {
            let functions = VMFunction::list_mut(context);
            // Fail before removing F if resolution did not establish custody.
            assert!(
                functions[self.original].owner_count() > 1,
                "function imports were not retained before the Tunables callout"
            );
            let replacement = functions[self.replacement].clone();
            drop(std::mem::replace(
                &mut functions[self.original],
                replacement,
            ));
            self.calls.fetch_add(1, Ordering::SeqCst);
            unsafe {
                self.base
                    .create_memories(context, module, styles, locations)
            }
        }
    }

    #[test]
    fn import_pins_precede_tunables_and_reexports_preserve_the_original_target() {
        use wasmer::sys::NativeEngineExt;
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Capture(drops.clone());
        let original = VMFunction::list(store.objects_mut().as_sys_mut()).len();
        let function = Function::new_typed(&mut store, move || -> i32 {
            std::hint::black_box(&probe);
            42
        });
        let replacement = VMFunction::list(store.objects_mut().as_sys_mut()).len();
        let _dummy = Function::new_typed(&mut store, |_: i64| -> i64 { 9 });
        let calls = Arc::new(AtomicUsize::new(0));
        store.engine_mut().set_tunables(ReplacingTunables {
            base: wasmer::sys::BaseTunables::new(),
            original,
            replacement,
            calls: calls.clone(),
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "env" "host" (func $host (result i32)))
            (memory 1)
            (func (export "run") (result i32) call $host)
            (export "again" (func $host)))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "host" => function } },
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        for name in ["run", "again"] {
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<(), i32>(&store, name)
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                42
            );
        }
        // Retire aliases only after all real calls. The initialized instance
        // alone still keeps its original import allocation alive.
        VMFunction::list_mut(store.objects_mut().as_sys_mut()).clear();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let retired = VMInstance::list_mut(store.objects_mut().as_sys_mut())
            .pop()
            .unwrap();
        drop(retired);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn a_pending_root_retains_its_removed_host_target_until_stack_retirement() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Capture(drops.clone());
        let observed = drops.clone();
        let index = VMFunction::list(store.objects_mut().as_sys_mut()).len();
        let env = FunctionEnv::new(&mut store, index);
        let function = Function::new_typed_with_env_async(
            &mut store,
            &env,
            move |env: AsyncFunctionEnvMut<usize>| {
                std::hint::black_box(&probe);
                let observed = observed.clone();
                async move {
                    let mut write = env.write().await;
                    let index = *write.data_mut();
                    let mut actual = write.as_store_mut();
                    assert!(
                        VMFunction::list(actual.objects_mut().as_sys_mut())[index].owner_count()
                            > 1,
                        "parked host target has no coroutine snapshot owner"
                    );
                    let _dummy = Function::new_typed(&mut actual, || -> i32 { 9 });
                    let functions = VMFunction::list_mut(actual.objects_mut().as_sys_mut());
                    let replacement = functions.pop().unwrap();
                    drop(std::mem::replace(&mut functions[index], replacement));
                    assert_eq!(observed.load(Ordering::SeqCst), 0);
                    drop(write);
                    std::future::pending::<Result<i32, RuntimeError>>().await
                }
            },
        );
        let store = store.into_async();
        let mut call = Box::pin(function.call_async(&store, vec![]));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let mut write = store.store().write();
        let fresh = Function::new_typed(&mut write.as_store_mut(), || -> i32 { 9 });
        assert_eq!(
            &*fresh.call(&mut write.as_store_mut(), &[]).unwrap(),
            &[Value::I32(9)]
        );
    }

    #[test]
    fn retries_keep_the_original_call_target_after_callback_slot_replacement() {
        for typed in [false, true] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Capture(drops.clone());
            let calls = Arc::new(AtomicUsize::new(0));
            let entered = calls.clone();
            let index = VMFunction::list(store.objects_mut().as_sys_mut()).len();
            let function = Function::new_typed(&mut store, move || -> i32 {
                std::hint::black_box(&probe);
                entered.fetch_add(1, Ordering::SeqCst);
                42
            });
            store.as_store_mut().on_called(move |mut actual| {
                let _dummy = Function::new_typed(&mut actual, |_: i64| -> i64 { 9 });
                let functions = VMFunction::list_mut(actual.objects_mut().as_sys_mut());
                let replacement = functions.pop().unwrap();
                drop(std::mem::replace(&mut functions[index], replacement));
                Ok(wasmer_types::OnCalledAction::InvokeAgain)
            });
            if typed {
                assert_eq!(
                    function
                        .typed::<(), i32>(&store)
                        .unwrap()
                        .call(&mut store)
                        .unwrap(),
                    42
                );
            } else {
                assert_eq!(&*function.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
            }
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn local_exports_keep_removed_instances_and_their_imports_alive() {
        for typed in [false, true] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Capture(drops.clone());
            let host = Function::new_typed(&mut store, move || -> i32 {
                std::hint::black_box(&probe);
                41
            });
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func $host (result i32)))
                (func (export "run") (result i32) call $host i32.const 1 i32.add))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "host" => host } },
            )
            .unwrap();
            let run = instance.exports.get_function("run").unwrap().clone();
            let typed_run = run.typed::<(), i32>(&store).unwrap();
            // Mutating away the local target pin must fail before freeing any
            // vmctx that a later baseline-only call would enter.
            assert!(
                VMInstance::list(store.objects_mut().as_sys_mut())
                    .last()
                    .unwrap()
                    .owner_count()
                    > 1,
                "a local exported target has no Instance allocation owner"
            );
            // Replace the host's Store descriptor without shifting the local
            // export's nonowning handle. The Instance import is its last owner.
            let _dummy = Function::new_typed(&mut store, || -> i32 { 9 });
            let functions = VMFunction::list_mut(store.objects_mut().as_sys_mut());
            let dummy = functions.pop().unwrap();
            drop(std::mem::replace(&mut functions[0], dummy));
            let retired = VMInstance::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            drop(retired);
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            if typed {
                assert_eq!(typed_run.call(&mut store).unwrap(), 42);
            } else {
                assert_eq!(&*run.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
            }
            VMFunction::list_mut(store.objects_mut().as_sys_mut()).clear();
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn wasm_imports_retain_a_removed_local_callee_instance() {
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Capture(drops.clone());
        let host = Function::new_typed(&mut store, move || -> i32 {
            std::hint::black_box(&probe);
            42
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "env" "callee" (func $callee (result i32)))
            (func (export "run") (result i32) call $callee))"#,
        )
        .unwrap();
        let first = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "callee" => host } },
        )
        .unwrap();
        let callee = first.exports.get_function("run").unwrap().clone();
        let first_target = VMFunction::list(store.objects_mut().as_sys_mut())
            .last()
            .unwrap()
            .id();
        let second = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "callee" => callee } },
        )
        .unwrap();
        assert!(
            VMInstance::list(store.objects_mut().as_sys_mut())[0].owner_count() > 1,
            "an imported local target has no Instance allocation owner"
        );
        assert!(VMFunction::list(store.objects_mut().as_sys_mut())[1].owner_count() > 1);
        let run = second
            .exports
            .get_typed_function::<(), i32>(&store, "run")
            .unwrap();
        // Preserve handle positions: replace both earlier descriptors, leaving
        // the later B export in its original row.
        for index in [0, 1] {
            let _dummy = Function::new_typed(&mut store, || -> i32 { 9 });
            let functions = VMFunction::list_mut(store.objects_mut().as_sys_mut());
            let dummy = functions.pop().unwrap();
            drop(std::mem::replace(&mut functions[index], dummy));
        }
        assert!(
            !VMFunction::list(store.objects_mut().as_sys_mut())
                .iter()
                .any(|f| f.id() == first_target)
        );
        let instances = VMInstance::list_mut(store.objects_mut().as_sys_mut());
        let second_owner = instances.pop().unwrap();
        let first_owner = instances.pop().unwrap();
        drop(first_owner);
        drop(second_owner);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(run.call(&mut store).unwrap(), 42);
        VMFunction::list_mut(store.objects_mut().as_sys_mut()).clear();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_active_local_call_keeps_its_removed_instance_until_return() {
        for typed in [false, true] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Capture(drops.clone());
            let env = FunctionEnv::new(&mut store, ());
            let host = Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut env: FunctionEnvMut<()>| -> i32 {
                    std::hint::black_box(&probe);
                    let mut actual = env.as_store_mut();
                    let objects = actual.objects_mut().as_sys_mut();
                    assert!(
                        VMInstance::list(objects)[0].owner_count() > 1,
                        "active local target has no Instance allocation owner"
                    );
                    assert!(
                        VMFunction::list(objects)[1].owner_count() > 1,
                        "active local target has no independent call snapshot"
                    );
                    let retired = VMInstance::list_mut(objects).pop().unwrap();
                    drop(retired);
                    VMFunction::list_mut(objects).clear();
                    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
                    41
                },
            );
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func $host (result i32)))
                (func (export "run") (result i32) call $host i32.const 1 i32.add))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "host" => host } },
            )
            .unwrap();
            let run = instance.exports.get_function("run").unwrap().clone();
            if typed {
                assert_eq!(
                    run.typed::<(), i32>(&store)
                        .unwrap()
                        .call(&mut store)
                        .unwrap(),
                    42
                );
            } else {
                assert_eq!(&*run.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn a_pending_local_call_keeps_its_removed_instance_until_retirement() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Capture(drops.clone());
        let env = FunctionEnv::new(&mut store, ());
        let host = Function::new_typed_with_env_async(
            &mut store,
            &env,
            move |env: AsyncFunctionEnvMut<()>| {
                std::hint::black_box(&probe);
                async move {
                    let mut write = env.write().await;
                    let mut actual = write.as_store_mut();
                    let objects = actual.objects_mut().as_sys_mut();
                    assert!(
                        VMInstance::list(objects)[0].owner_count() > 1,
                        "parked local target has no Instance allocation owner"
                    );
                    assert!(
                        VMFunction::list(objects)[1].owner_count() > 1,
                        "parked local target has no independent call snapshot"
                    );
                    let retired = VMInstance::list_mut(objects).pop().unwrap();
                    drop(retired);
                    VMFunction::list_mut(objects).clear();
                    drop(write);
                    std::future::pending::<Result<i32, RuntimeError>>().await
                }
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "env" "host" (func $host (result i32)))
            (func (export "run") (result i32) call $host))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "host" => host } },
        )
        .unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let store = store.into_async();
        let mut call = Box::pin(run.call_async(&store, vec![]));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let mut write = store.store().write();
        let fresh = Function::new_typed(&mut write.as_store_mut(), || -> i32 { 9 });
        assert_eq!(
            &*fresh.call(&mut write.as_store_mut(), &[]).unwrap(),
            &[Value::I32(9)]
        );
    }
    #[test]
    fn table_callbacks_capture_their_live_instance_before_removing_its_rows() {
        for typed in [false, true] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = Capture(drops.clone());
            let env = FunctionEnv::new(&mut store, ());
            let host = Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut env: FunctionEnvMut<()>| -> i32 {
                    std::hint::black_box(&probe);
                    let mut actual = env.as_store_mut();
                    let objects = actual.objects_mut().as_sys_mut();
                    assert!(
                        VMFunction::list(objects).last().unwrap().owner_count() > 1,
                        "active table callback has no independent call snapshot"
                    );
                    VMFunction::list_mut(objects).clear();
                    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
                    41
                },
            );
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func $host (result i32)))
                (table (export "callbacks") 4 funcref)
                (func $anchor (result i32) i32.const 0)
                (func $other (result i32) i32.const 42)
                (func $callback (result i32) call $host i32.const 1 i32.add)
                (elem (i32.const 0) $anchor $other $callback))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "host" => host } },
            )
            .unwrap();
            let table = instance.exports.get_table("callbacks").unwrap();
            let Value::FuncRef(Some(anchor)) = table.get(&mut store, 0).unwrap() else {
                panic!("anchor")
            };
            assert!(
                VMInstance::list(store.objects_mut().as_sys_mut())[0].owner_count() > 1,
                "table lookup did not retain its current Instance allocation"
            );
            // With the source row gone, the anchor's local allocation pin must
            // resolve a DIFFERENT private function in the same allocation.
            let retired = VMInstance::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            drop(retired);
            let Value::FuncRef(Some(other)) = table.get(&mut store, 1).unwrap() else {
                panic!("other")
            };
            assert_eq!(
                other
                    .typed::<(), i32>(&store)
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                42
            );
            let local_ids: Vec<_> = VMFunction::list(store.objects_mut().as_sys_mut())
                .iter()
                .filter(|target| target.instance_owner_count().is_some())
                .map(VMFunction::id)
                .collect();
            // The next lookup must reach A through B's immutable import pin,
            // after every A descriptor has also left the Store function vector.
            let dependent = Module::new(
                &store,
                r#"(module
                (import "env" "anchor" (func (result i32)))
                (table (export "unused") 0 funcref))"#,
            )
            .unwrap();
            let _dependent = Instance::new(
                &mut store,
                &dependent,
                &imports! { "env" => { "anchor" => anchor } },
            )
            .unwrap();
            VMFunction::list_mut(store.objects_mut().as_sys_mut())
                .retain(|target| !local_ids.contains(&target.id()));
            let Value::FuncRef(Some(callback)) = table.get(&mut store, 2).unwrap() else {
                panic!("callback")
            };
            assert!(
                VMFunction::list(store.objects_mut().as_sys_mut())
                    .last()
                    .unwrap()
                    .instance_owner_count()
                    .is_some_and(|count| count > 1),
                "import-DAG lookup did not independently retain the callback allocation"
            );
            let dependent = VMInstance::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            drop(dependent);
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            assert!(matches!(
                table.get(&mut store, 3),
                Some(Value::FuncRef(None))
            ));
            if typed {
                assert_eq!(
                    callback
                        .typed::<(), i32>(&store)
                        .unwrap()
                        .call(&mut store)
                        .unwrap(),
                    42
                );
            } else {
                assert_eq!(&*callback.call(&mut store, &[]).unwrap(), &[Value::I32(42)]);
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn pending_table_callbacks_keep_their_removed_instance_until_retirement() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Capture(drops.clone());
        let env = FunctionEnv::new(&mut store, ());
        let host = Function::new_typed_with_env_async(
            &mut store,
            &env,
            move |env: AsyncFunctionEnvMut<()>| {
                std::hint::black_box(&probe);
                async move {
                    let mut write = env.write().await;
                    let mut actual = write.as_store_mut();
                    let objects = actual.objects_mut().as_sys_mut();
                    assert!(
                        VMInstance::list(objects)[0].owner_count() > 1,
                        "pending table callback has no Instance allocation owner"
                    );
                    let retired = VMInstance::list_mut(objects).pop().unwrap();
                    drop(retired);
                    VMFunction::list_mut(objects).clear();
                    drop(write);
                    std::future::pending::<Result<i32, RuntimeError>>().await
                }
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "env" "host" (func $host (result i32)))
            (table (export "callbacks") 1 funcref)
            (func $callback (result i32) call $host)
            (elem (i32.const 0) $callback))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "env" => { "host" => host } },
        )
        .unwrap();
        let table = instance.exports.get_table("callbacks").unwrap();
        let Value::FuncRef(Some(callback)) = table.get(&mut store, 0).unwrap() else {
            panic!("callback")
        };
        assert!(
            VMInstance::list(store.objects_mut().as_sys_mut())[0].owner_count() > 1,
            "table lookup did not retain its current Instance allocation"
        );
        let store = store.into_async();
        let mut call = Box::pin(callback.call_async(&store, vec![]));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let mut write = store.store().write();
        let fresh = Function::new_typed(&mut write.as_store_mut(), || -> i32 { 9 });
        assert_eq!(
            &*fresh.call(&mut write.as_store_mut(), &[]).unwrap(),
            &[Value::I32(9)]
        );
    }

    #[test]
    fn table_lookup_checks_instance_origin_and_refuses_an_empty_owner_inventory() {
        use wasmer::sys::vm::{StoreHandle, VMExtern, VMTable};
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
            (table (export "callbacks") 1 funcref)
            (func $callback (result i32) i32.const 42)
            (elem (i32.const 0) $callback))"#,
        )
        .unwrap();
        let instance = Instance::new(&mut store, &module, &Imports::new()).unwrap();
        let table = instance.exports.get_table("callbacks").unwrap();
        let original = store.id();
        let mut other = Store::default();
        let other_epoch = other.id();
        other.objects_mut().as_sys_mut().set_id(original);
        let copied = VMTable::list(store.objects_mut().as_sys_mut())[0]
            .copy_on_write()
            .unwrap();
        let handle = StoreHandle::new(other.objects_mut().as_sys_mut(), copied);
        let raw = VMExtern::Table(handle.internal_handle());
        let wasmer::Extern::Table(other_table) =
            wasmer::VMExternToExtern::to_extern(raw, &mut other)
        else {
            panic!("table")
        };
        let moved = VMInstance::list_mut(store.objects_mut().as_sys_mut())
            .pop()
            .unwrap();
        VMInstance::list_mut(other.objects_mut().as_sys_mut()).push(moved);
        // Equal IDs isolate the actual Store-address check. No wrong-context
        // pointer is dereferenced to reject this transplanted Instance row.
        assert!(
            other_table.get(&mut other, 0).is_none(),
            "table lookup accepted a transplanted Instance"
        );
        let moved = VMInstance::list_mut(other.objects_mut().as_sys_mut())
            .pop()
            .unwrap();
        VMInstance::list_mut(store.objects_mut().as_sys_mut()).push(moved);
        drop(other);

        let copied = VMTable::list(store.objects_mut().as_sys_mut())[0]
            .copy_on_write()
            .unwrap();
        store.objects_mut().as_sys_mut().set_id(other_epoch);
        let handle = StoreHandle::new(store.objects_mut().as_sys_mut(), copied);
        let raw = VMExtern::Table(handle.internal_handle());
        let wasmer::Extern::Table(rebound) = wasmer::VMExternToExtern::to_extern(raw, &mut store)
        else {
            panic!("table")
        };
        // The current table handle and actual address match; only the immutable
        // Instance birth identity can reject rebinding its old code metadata.
        assert!(
            rebound.get(&mut store, 0).is_none(),
            "table lookup rebound an old Instance epoch"
        );
        store.objects_mut().as_sys_mut().set_id(original);
        let Value::FuncRef(Some(callback)) = table.get(&mut store, 0).unwrap() else {
            panic!("callback")
        };
        assert_eq!(
            callback
                .typed::<(), i32>(&store)
                .unwrap()
                .call(&mut store)
                .unwrap(),
            42
        );
        VMFunction::list_mut(store.objects_mut().as_sys_mut()).clear();
        VMInstance::list_mut(store.objects_mut().as_sys_mut()).clear();
        // With no current owner candidates the API refuses the raw entry without
        // dereferencing its freed anyfunc. This is not a pointer-ABA guarantee.
        assert!(table.get(&mut store, 0).is_none());
    }

    #[test]
    fn table_host_aliases_keep_the_original_host_calling_convention() {
        for dynamic in [false, true] {
            let mut store = Store::default();
            let host = if dynamic {
                Function::new(&mut store, FunctionType::new([], [Type::I32]), |_| {
                    Ok(vec![Value::I32(42)])
                })
            } else {
                Function::new_typed(&mut store, || -> i32 { 42 })
            };
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "host" (func $host (result i32)))
                (table (export "callbacks") 1 funcref)
                (elem (i32.const 0) $host))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "host" => host } },
            )
            .unwrap();
            let table = instance.exports.get_table("callbacks").unwrap();
            let Value::FuncRef(Some(callback)) = table.get(&mut store, 0).unwrap() else {
                panic!("callback")
            };
            assert!(
                !VMFunction::list(store.objects_mut().as_sys_mut())
                    .last()
                    .unwrap()
                    .host_data()
                    .is::<()>(),
                "a host table alias gained the Wasm-only memory-zero parameter"
            );
            assert_eq!(
                callback
                    .typed::<(), i32>(&store)
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                42
            );
            let module = Module::new(
                &store,
                r#"(module
                (import "env" "callback" (func $callback (result i32)))
                (func (export "run") (result i32) call $callback))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "env" => { "callback" => callback } },
            )
            .unwrap();
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "run")
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                42
            );
        }
    }
}
