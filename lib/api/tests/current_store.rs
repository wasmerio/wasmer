//! Lending the executing store to code that cannot be handed a reference to
//! it: [`StoreMut::parked`] + [`Store::with_current`].

use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

use wasmer::*;

/// Stands in for the foreign code an embedder calls into — a JS engine's
/// allocator hook, a C callback — which reaches the host again with no store
/// of its own and no way to have been handed one.
fn called_through_foreign_frames<R>(f: impl FnOnce() -> R) -> R {
    f()
}

/// A memory the host can grow, shared where the backend allows it (the case
/// the API exists for) and plain where it does not.
fn growable_memory(store: &mut Store) -> Memory {
    let shared = MemoryType::new(Pages(1), Some(Pages(4)), true);
    let owned = MemoryType::new(Pages(1), Some(Pages(4)), false);
    Memory::new(&mut *store, shared)
        .or_else(|_| Memory::new(&mut *store, owned))
        .expect("create memory")
}

/// Calls `"run"` on a module whose only body is a call to the `"host"."run"`
/// import, so `host` runs with a store the runtime installed for the call.
fn run_host_function(store: &mut Store, host: Function) -> Result<(), String> {
    let wat = r#"(module
        (func $host (import "host" "run"))
        (func (export "run") (call $host))
    )"#;
    let module = Module::new(&*store, wat).map_err(|e| format!("{e:?}"))?;
    let imports = imports! { "host" => { "run" => host } };
    let instance = Instance::new(&mut *store, &module, &imports).map_err(|e| format!("{e:?}"))?;
    instance
        .exports
        .get_typed_function::<(), ()>(&*store, "run")
        .map_err(|e| format!("{e:?}"))?
        .call(&mut *store)
        .map_err(|e| format!("{e:?}"))
}

#[derive(Default)]
struct Report {
    /// Whether the store was lent out before anyone parked it.
    lent_unparked: bool,
    /// Whether it was lent out from inside the park.
    lent_parked: bool,
    /// Whether it was lent out again once the park ended.
    lent_after_park: bool,
    /// Whether the store lent out is the one the host function is running
    /// under, rather than some other store on this thread.
    same_store: bool,
    /// Size the memory reported before the lent store grew it.
    size_before_grow: Option<Pages>,
}

struct HostEnv {
    memory: Memory,
    report: Report,
}

#[engine_test]
fn store_is_lent_out_only_while_parked() -> Result<(), String> {
    let mut store = Store::default();
    let memory = growable_memory(&mut store);
    let env = FunctionEnv::new(
        &mut store,
        HostEnv {
            memory: memory.clone(),
            report: Report::default(),
        },
    );

    let host =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<HostEnv>| {
            let memory = env.data().memory.clone();
            let store_id = env.objects_mut().id();

            // The host function is holding the store, so nobody else may have it.
            let lent_unparked =
                called_through_foreign_frames(|| Store::with_current(|_| ()).is_some());

            let mut same_store = false;
            let size_before_grow = Some(memory.view(&env).size());
            let lent_parked = env
                .as_store_mut()
                .parked(|| {
                    called_through_foreign_frames(|| {
                        Store::with_current(|store| {
                            same_store = store.objects_mut().id() == store_id
                                && memory.is_from_store(&*store);
                            // The whole point: reach the store's ordinary API from
                            // code that was never given a store.
                            memory.grow(store, Pages(1)).expect("grow the lent memory");
                        })
                    })
                })
                .is_some();

            // The park is over; the store belongs to this function again.
            let lent_after_park =
                called_through_foreign_frames(|| Store::with_current(|_| ()).is_some());

            env.data_mut().report = Report {
                lent_unparked,
                lent_parked,
                lent_after_park,
                same_store,
                size_before_grow,
            };
        });

    run_host_function(&mut store, host)?;

    let report = &env.as_ref(&store).report;
    assert!(
        !report.lent_unparked,
        "a store still held by the host function must not be lent out"
    );

    if !report.lent_parked {
        // Backends that do not track an executing store (v8, js) lend nothing.
        // What matters is that they say so instead of panicking or handing
        // back some other store, which the assertions above already cover.
        return Ok(());
    }

    assert!(
        report.same_store,
        "the store lent out must be the one the call is running under"
    );
    assert!(
        !report.lent_after_park,
        "the lend must end with the park, not outlive it"
    );
    assert_eq!(
        Some(memory.view(&store).size()),
        report.size_before_grow.map(|before| before + Pages(1)),
        "a grow through the lent store must be visible to its owner"
    );

    Ok(())
}

#[engine_test]
fn parks_nest() -> Result<(), String> {
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, Vec::<bool>::new());

    let host =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Vec<bool>>| {
            let mut lends = Vec::new();
            env.as_store_mut().parked(|| {
                Store::with_current(|lent| {
                    // The lent borrow is itself a borrow: until it is parked in
                    // turn, it is the one thing standing in the way of a
                    // further lend.
                    lends.push(Store::with_current(|_| ()).is_some());
                    lent.parked(|| lends.push(Store::with_current(|_| ()).is_some()));
                    // ...and unparking it puts it back in the way.
                    lends.push(Store::with_current(|_| ()).is_some());
                });
            });
            *env.data_mut() = lends;
        });

    run_host_function(&mut store, host)?;

    let lends = env.as_ref(&store);
    if lends.is_empty() {
        // Backend does not track an executing store; nothing was lent at all.
        return Ok(());
    }
    assert_eq!(
        lends.as_slice(),
        [false, true, false],
        "a lent store is lendable again only while it is itself parked"
    );

    Ok(())
}

/// A park that ends by unwinding must hand the borrow back, or the host
/// function that caught the panic would keep using a store that has been
/// declared lendable.
///
/// Native only: panics abort on `wasm32-unknown-unknown`, so there is no
/// unwinding there to observe.
#[cfg(not(target_arch = "wasm32"))]
#[engine_test]
fn a_park_that_unwinds_gives_the_borrow_back() -> Result<(), String> {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, None::<bool>);

    let host =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Option<bool>>| {
            // The panic below is the point of the test, not a failure; keep it
            // from printing a scary backtrace in the middle of a passing run.
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let unwound = catch_unwind(AssertUnwindSafe(|| {
                env.as_store_mut().parked(|| panic!("boom"))
            }))
            .is_err();
            std::panic::set_hook(hook);
            assert!(unwound, "the park must not swallow the panic");

            // `env` still holds the store, exactly as it did before the park.
            *env.data_mut() = Some(Store::with_current(|_| ()).is_some());
        });

    run_host_function(&mut store, host)?;

    assert_eq!(
        *env.as_ref(&store),
        Some(false),
        "a store whose park unwound must not stay lendable"
    );

    Ok(())
}

/// An idle store — one no call is executing — is nobody's to take until its
/// owner parks it. Parking is what an embedder does to lend a store to a
/// callback that runs outside any call at all.
#[engine_test]
fn an_idle_store_is_lent_only_once_parked() -> Result<(), String> {
    let mut store = Store::default();
    let store_id = store.id();

    assert!(
        Store::with_current(|_| ()).is_none(),
        "an idle store is not lendable on its own"
    );

    let lent = store
        .as_store_mut()
        .parked(|| Store::with_current(|store| store.objects_mut().id()));
    assert_eq!(lent, Some(store_id), "parking lends the store it parked");

    assert!(
        Store::with_current(|_| ()).is_none(),
        "and the lend ends with the park"
    );

    Ok(())
}
