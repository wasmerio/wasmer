//! The store a host function runs with.
//!
//! A host function on this backend is a JavaScript function, which anything
//! can call: a guest of its store, a guest whose export was called with
//! another store, a guest of another store that imports it, or JavaScript,
//! also once its store is gone. So it keeps no pointer to its store: the call
//! it runs in lends it one. Every call into a guest made with a store lends
//! it, from the borrow the call was made with, to the host functions it
//! reaches, until it returns: `Function::call`, `TypedFunction::call`,
//! `Instance::new` for the start function, and `Function::call_async` for the
//! slice of its guest that it runs. A host function runs with the store the
//! innermost call lends, if that is its own. Otherwise it fails, and touches
//! nothing. In a slice of a guest of `call_async` that JavaScript resumed,
//! which no call lends anything, it runs with the store the guest holds.
//!
//! A host function holds the store it runs with until it returns, and no
//! other host function can run with it meanwhile. The calls it makes lend the
//! store again, from the borrow it has, to the host functions they reach. A
//! host function it reaches some other way, such as through a guest's export
//! that JavaScript calls as a function, finds the store held, and fails as
//! one called outside any call does. Otherwise both would use the store at
//! once.
//!
//! The lends are a stack. Each call puts it back as it found it once its
//! guest returns, however it returns: a JavaScript exception, such as the one
//! the engine throws when it runs out of stack, unwinds through Rust frames
//! without running them, and so leaves the lends of the calls it unwound
//! behind, for the call that catches it to take off. The hold of a host
//! function it unwinds is in the lend of the call the host function ran in,
//! and goes with it. Until then, a guest that catches the exception itself
//! cannot call its host functions.
//!
//! The depth of the stack is that of the calls nested in host functions on
//! the thread, which is bounded: see [`MAX_DEPTH`].

use std::cell::Cell;

use wasmer_types::{StoreId, TrapCode};

use crate::{AsStoreMut, BackendTrap, RuntimeError, StoreInner, StoreMut, js::error::Trap};

/// How deeply calls into guests can nest in host functions on one thread. A
/// call deeper than that fails with a stack overflow before it enters its
/// guest, so that a recursion through host functions returns through every
/// frame. Otherwise the engine runs out of stack first, and its exception
/// unwinds through Rust frames without running them: the destructors of the
/// host functions it unwinds never run, nor their code after the call. With
/// its default stack, V8 runs out after about 150 levels of a recursion
/// through a host function in a debug build, and 250 in a release one; other
/// engines go deeper.
///
/// The bound is a mitigation, not a guarantee: it counts calls, not stack.
/// A guest with bigger frames, or one that recurses within itself between
/// its calls to host functions, can run the engine out of stack in fewer
/// levels. With frames of 4 KB (500 `i64` locals live across the call), V8
/// runs out after 97 levels in a debug build; with 32 KB, after 27 in a
/// release one.
pub(crate) const MAX_DEPTH: usize = 100;

/// A store lent: its raw id, and the store, or null once it is dropped.
type Lent = Cell<(usize, *mut StoreInner)>;

/// The stores lent on a thread: `stack[..depth]`, innermost last, and
/// whether a host function holds each. Cells rather than a `RefCell`, which a
/// host function would check on every call.
struct Lends {
    depth: Cell<usize>,
    stack: [Lent; MAX_DEPTH],
    held: [Cell<bool>; MAX_DEPTH],
}

thread_local! {
    static LENDS: Lends = const {
        Lends {
            depth: Cell::new(0),
            stack: [const { Cell::new((0, std::ptr::null_mut())) }; MAX_DEPTH],
            held: [const { Cell::new(false) }; MAX_DEPTH],
        }
    };
}

/// A lend of a store, taken back when dropped.
pub(crate) struct Lend {
    depth: usize,
}

/// Lends `store` to the host functions the call about to enter a guest
/// reaches, until the returned guard is dropped. Fails if calls nest too
/// deeply already.
pub(crate) fn lend(store: &mut StoreMut<'_>) -> Result<Lend, RuntimeError> {
    let id = store.objects_mut().id();
    lend_raw(id, &mut *store.inner)
}

/// [`lend`], for a store `ptr` points to, which must stay valid, and used by
/// nothing else, until the guard is dropped.
pub(crate) fn lend_raw(id: StoreId, ptr: *mut StoreInner) -> Result<Lend, RuntimeError> {
    LENDS.with(|lends| {
        let depth = lends.depth.get();
        let lent = lends.stack.get(depth).ok_or_else(too_deep)?;
        lent.set((id.as_raw().get(), ptr));
        lends.held[depth].set(false);
        lends.depth.set(depth + 1);
        Ok(Lend { depth })
    })
}

impl Drop for Lend {
    fn drop(&mut self) {
        // Also the lends of calls nested in this one that a JavaScript
        // exception unwound without running their frames.
        let _ = LENDS.try_with(|lends| lends.depth.set(self.depth));
    }
}

/// A host function's hold on the store it runs with: no other host function
/// can run with the store until it is dropped.
pub(crate) enum Hold {
    /// The flag of the lend held, in the thread's lends.
    Lend(*const Cell<bool>),
    /// The store a guest of `call_async` that JavaScript resumed holds.
    #[cfg(feature = "experimental-async")]
    Running(crate::js::jspi::Held),
}

impl Drop for Hold {
    #[inline]
    fn drop(&mut self) {
        match *self {
            // SAFETY: the flag is in the thread's lends, which last as long
            // as the thread, and a `Hold` stays on its thread. The lend is
            // still there: the call it is of returns after the host function
            // does, and an exception that unwinds the host function skips
            // this.
            Self::Lend(held) => unsafe { (*held).set(false) },
            #[cfg(feature = "experimental-async")]
            Self::Running(_) => {}
        }
    }
}

/// The store of `id`, the raw id of a store, for one of its host functions
/// to run with, and its hold on it: the one the innermost call lends, if that
/// is the store `id` and no other host function holds it. Inlined into the
/// host functions, which are generic, and so built in the embedder's crate.
#[inline]
pub(crate) fn hold(id: usize) -> Result<(*mut StoreInner, Hold), RuntimeError> {
    let top = LENDS.with(|lends| {
        let at = lends.depth.get().checked_sub(1)?;
        let (lent, store) = lends.stack[at].get();
        Some(if lent != id || store.is_null() {
            Err(not_lent())
        } else if lends.held[at].replace(true) {
            Err(held())
        } else {
            Ok((store, Hold::Lend(&lends.held[at])))
        })
    });
    match top {
        Some(result) => result,
        None => {
            #[cfg(feature = "experimental-async")]
            if let Some(running) = crate::js::jspi::hold_running(id) {
                return running.map(|(store, held)| (store, Hold::Running(held)));
            }
            Err(not_lent())
        }
    }
}

/// Forgets the store `id`, which is being dropped, wherever it is lent: only
/// a lend a JavaScript exception left behind can still be there.
pub(crate) fn forget(id: StoreId) {
    let id = id.as_raw().get();
    let _ = LENDS.try_with(|lends| {
        for lent in &lends.stack[..lends.depth.get()] {
            if lent.get().0 == id {
                lent.set((id, std::ptr::null_mut()));
            }
        }
    });
}

#[cold]
fn not_lent() -> RuntimeError {
    RuntimeError::new("a host function can only run in a call made with its own store")
}

/// The error of a host function whose store another host function holds.
#[cold]
pub(crate) fn held() -> RuntimeError {
    RuntimeError::new(
        "a host function cannot run while another host function uses its store: \
         a host function calls guests with `Function::call`, not through JavaScript",
    )
}

fn too_deep() -> RuntimeError {
    let code = TrapCode::StackOverflow;
    let message = format!(
        "{}: calls into guests nest in host functions more than {MAX_DEPTH} deep",
        code.message()
    );
    let source = BackendTrap::Js(Trap::user(message.into()));
    RuntimeError::new_from_source(source, Vec::new(), Some(code))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    #[cfg(feature = "experimental-async")]
    use std::future::Future;

    use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use crate::{
        Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, Store, Type, Value,
        imports, js::utils::convert::AsJs,
    };

    thread_local! {
        /// How many times a host function below ran.
        static RAN: Cell<u32> = const { Cell::new(0) };
        /// A store a host function below drops, if it is there.
        static PARKED: RefCell<Option<Store>> = const { RefCell::new(None) };
    }

    fn lent() -> usize {
        LENDS.with(|lends| lends.depth.get())
    }

    /// A host function's body: counts the call, and drops the parked store.
    fn body<T>(value: T) -> T {
        RAN.set(RAN.get() + 1);
        drop(PARKED.with(|parked| parked.borrow_mut().take()));
        value
    }

    /// A host function of `store` of each kind, with the type of its one
    /// parameter and result: dynamic and typed, with and without an
    /// environment, on the numeric lane (`i32`) and off it (`i64`).
    fn host_functions(store: &mut Store) -> Vec<(Function, Type)> {
        let env = FunctionEnv::new(store, ());
        let dynamic = |store: &mut Store, ty: Type| {
            let signature = FunctionType::new(vec![ty], vec![ty]);
            let function = Function::new_with_env(store, &env, signature, |_env, args| {
                Ok(vec![body(args[0].clone())])
            });
            (function, ty)
        };
        vec![
            dynamic(store, Type::I32),
            dynamic(store, Type::I64),
            (Function::new_typed(store, |x: i32| body(x)), Type::I32),
            (
                Function::new_typed_with_env(store, &env, |_env: FunctionEnvMut<()>, x: i64| {
                    body(x)
                }),
                Type::I64,
            ),
        ]
    }

    /// The export of a guest of `store` that calls `function` with its
    /// argument, of type `ty`, and returns what it returns.
    fn guest(store: &mut Store, function: Function, ty: Type) -> Function {
        let ty = match ty {
            Type::I32 => "i32",
            _ => "i64",
        };
        let wat = format!(
            r#"(module
              (import "env" "f" (func $f (param {ty}) (result {ty})))
              (func (export "run") (param {ty}) (result {ty}) local.get 0 call $f))"#
        );
        let module = Module::new(store, wat).unwrap();
        let imports = imports! { "env" => { "f" => function } };
        let instance = Instance::new(store, &module, &imports).unwrap();
        instance.exports.get_function("run").unwrap().clone()
    }

    fn argument(ty: Type) -> Value {
        match ty {
            Type::I32 => Value::I32(3),
            _ => Value::I64(3),
        }
    }

    /// A guest's export can be called with any store on this backend, but
    /// its host functions run only with their own store: called with another
    /// one, they fail without running, so they cannot drop their own, which
    /// nothing borrows, while they use it. With their own store, they run.
    #[wasm_bindgen_test]
    fn a_host_function_runs_only_in_a_call_made_with_its_own_store() {
        let mut own = Store::default();
        for (function, ty) in host_functions(&mut own) {
            let run = guest(&mut own, function, ty);
            RAN.set(0);
            let results = run.call(&mut own, &[argument(ty)]).unwrap();
            assert_eq!(results.to_vec(), vec![argument(ty)]);
            assert_eq!(RAN.get(), 1);

            PARKED.with(|parked| *parked.borrow_mut() = Some(own));
            let mut other = Store::default();
            let error = run.call(&mut other, &[argument(ty)]).unwrap_err();
            assert!(error.message().contains("its own store"), "{ty}: {error}");
            assert_eq!(RAN.get(), 1, "{ty}: the host function ran");
            own = PARKED
                .with(|parked| parked.borrow_mut().take())
                .expect("dropped");
            if ty == Type::I32 {
                let typed = run.typed::<i32, i32>(&own).unwrap();
                assert!(typed.call(&mut other, 3).is_err());
                assert_eq!(typed.call(&mut own, 3).unwrap(), 3);
            }
            assert_eq!(lent(), 0);
        }
    }

    /// Called by JavaScript outside any call, a host function fails without
    /// running, also once its store is gone.
    #[wasm_bindgen_test]
    fn a_host_function_called_outside_a_call_fails() {
        let mut store = Store::default();
        let functions = host_functions(&mut store)
            .into_iter()
            .map(|(function, ty)| (function.as_jsvalue(&store), ty))
            .collect::<Vec<_>>();
        let call = |function: &JsValue, ty: Type| {
            let function: &js_sys::Function = function.unchecked_ref();
            let argument = match ty {
                Type::I32 => JsValue::from(3),
                _ => JsValue::from(js_sys::BigInt::from(3)),
            };
            function.call1(&JsValue::UNDEFINED, &argument)
        };
        RAN.set(0);
        for (function, ty) in &functions {
            assert!(call(function, *ty).is_err(), "{ty}");
        }
        drop(store);
        for (function, ty) in &functions {
            assert!(call(function, *ty).is_err(), "{ty}");
        }
        assert_eq!(RAN.get(), 0);
    }

    thread_local! {
        static DEPTH: Cell<usize> = const { Cell::new(0) };
        static DEEPEST: Cell<usize> = const { Cell::new(0) };
    }

    /// A guest that calls a host function that calls the guest again, and
    /// so on, fails once calls nest [`MAX_DEPTH`] deep, before the engine runs
    /// out of stack: every host function goes on after its call fails, and
    /// nothing is left lent. Again and again.
    #[wasm_bindgen_test]
    fn a_recursion_through_host_functions_stops_at_the_bound() {
        #[derive(Clone)]
        struct Env {
            run: Option<Function>,
        }
        let mut store = Store::default();
        let env = FunctionEnv::new(&mut store, Env { run: None });
        let recurse = Function::new_typed_with_env(
            &mut store,
            &env,
            |mut env: FunctionEnvMut<Env>, x: i32| -> Result<i32, RuntimeError> {
                DEPTH.set(DEPTH.get() + 1);
                DEEPEST.set(DEEPEST.get().max(DEPTH.get()));
                let run = env.data().run.clone().unwrap();
                let result = run.call(&mut env.as_store_mut(), &[Value::I32(x + 1)]);
                DEPTH.set(DEPTH.get() - 1);
                Ok(result?[0].unwrap_i32())
            },
        );
        let run = guest(&mut store, recurse, Type::I32);
        env.as_mut(&mut store).run = Some(run.clone());
        for _ in 0..3 {
            DEEPEST.set(0);
            let error = run.call(&mut store, &[Value::I32(0)]).unwrap_err();
            assert_eq!(
                error.clone().to_trap(),
                Some(TrapCode::StackOverflow),
                "{error}"
            );
            // One host function per call that entered the guest.
            assert_eq!(DEEPEST.get(), MAX_DEPTH);
            assert_eq!(DEPTH.get(), 0);
            assert_eq!(lent(), 0);
        }
    }

    #[cfg(feature = "experimental-async")]
    thread_local! {
        /// What a host function of another store hands a test below.
        static OTHER: RefCell<Option<(Function, crate::StoreAsync)>> =
            const { RefCell::new(None) };
        /// The call that host function starts, for the test to wait for.
        #[allow(clippy::type_complexity)]
        static STARTED: RefCell<
            Option<std::pin::Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>>,
        > = const { RefCell::new(None) };
    }

    /// `call_async` lends its store for the slice of the guest it runs, also
    /// from a host function of another store: the guest's host functions run
    /// with their own store there.
    #[cfg(feature = "experimental-async")]
    #[wasm_bindgen_test]
    async fn a_call_async_made_in_a_host_function_lends_its_store() {
        let mut other = Store::default();
        let (function, ty) = host_functions(&mut other).remove(0);
        let run_other = guest(&mut other, function, ty);
        OTHER.with(|slot| *slot.borrow_mut() = Some((run_other, other.into_async())));

        let mut store = Store::default();
        let start = Function::new_typed(&mut store, |x: i32| -> i32 {
            let (run, store) = OTHER.with(|slot| slot.borrow_mut().take()).unwrap();
            let mut call = Box::pin(run.call_async(&store, vec![Value::I32(x)]));
            // Runs the guest's slice, which does not wait.
            let waker = futures::task::noop_waker();
            let poll = call
                .as_mut()
                .poll(&mut std::task::Context::from_waker(&waker));
            assert!(poll.is_pending());
            STARTED.with(|slot| *slot.borrow_mut() = Some(call));
            x
        });
        let run = guest(&mut store, start, Type::I32);
        RAN.set(0);
        assert_eq!(
            run.call(&mut store, &[Value::I32(3)]).unwrap().to_vec(),
            vec![Value::I32(3)]
        );
        assert_eq!(RAN.get(), 1);
        let call = STARTED.with(|slot| slot.borrow_mut().take()).unwrap();
        assert_eq!(call.await.unwrap().to_vec(), vec![Value::I32(3)]);
    }

    /// A slice of a guest of `call_async` that JavaScript resumes, once
    /// what the guest waited for is ready, runs with no call that lends a
    /// store: its host functions run with the store the guest holds.
    #[cfg(feature = "experimental-async")]
    #[wasm_bindgen_test]
    async fn a_resumed_guest_runs_its_host_functions_with_its_store() {
        let mut store = Store::default();
        let (function, _) = host_functions(&mut store).remove(0);
        let wait = Function::new_typed_async(&mut store, async || -> i32 {
            let resolved = js_sys::Promise::resolve(&JsValue::from(3));
            wasm_bindgen_futures::JsFuture::from(resolved)
                .await
                .unwrap();
            3
        });
        let module = Module::new(
            &store,
            r#"(module
              (import "env" "wait" (func $wait (result i32)))
              (import "env" "f" (func $f (param i32) (result i32)))
              (func (export "run") (result i32) call $wait call $f))"#,
        )
        .unwrap();
        let imports = imports! { "env" => { "wait" => wait, "f" => function } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let store = store.into_async();
        RAN.set(0);
        let results = run.call_async(&store, vec![]).await.unwrap();
        assert_eq!(results.to_vec(), vec![Value::I32(3)]);
        assert_eq!(RAN.get(), 1);
        assert_eq!(lent(), 0);
    }

    /// A JavaScript exception that unwinds through Rust frames without
    /// running them leaves their lends behind: the call that catches it
    /// takes them off when it returns. A store dropped while one is left
    /// behind is not lent from it any more.
    #[wasm_bindgen_test]
    fn the_lends_an_exception_leaves_behind_are_taken_off() {
        let mut outer_store = Store::default();
        let mut other = Store::default();
        let id = other.id().as_raw().get();
        let lend_other = lend(&mut other.as_store_mut()).unwrap();
        std::mem::forget(lend_other);
        let outer = lend(&mut outer_store.as_store_mut()).unwrap();

        // Lends the store, and throws while it is lent.
        fn unwinds(id: StoreId, ptr: *mut StoreInner) {
            let _lend = lend_raw(id, ptr).unwrap();
            wasm_bindgen::throw_str("unwound")
        }
        let ptr: *mut StoreInner = &mut *other.as_store_mut().inner;
        let other_id = other.id();
        let unwound = Closure::wrap(Box::new(move || unwinds(other_id, ptr)) as Box<dyn Fn()>);
        let unwound: &js_sys::Function = unwound.as_ref().unchecked_ref();
        assert!(unwound.call0(&JsValue::UNDEFINED).is_err());
        assert_eq!(lent(), 3);
        assert!(super::hold(id).is_ok());

        drop(outer);
        assert_eq!(lent(), 1);
        drop(other);
        assert!(super::hold(id).is_err());
        LENDS.with(|lends| lends.depth.set(0));
    }

    thread_local! {
        /// A guest's export that calls a host function, that export as
        /// JavaScript calls it, and the type of its parameter.
        static EXPORT: RefCell<Option<(Function, js_sys::Function, Type)>> =
            const { RefCell::new(None) };
        /// How each call of [`reenter`] went: its error, if it failed, and
        /// how many times the host function ran.
        static SEEN: RefCell<Vec<(Option<String>, u32)>> = const { RefCell::new(Vec::new()) };
    }

    /// The body of a host function that calls the export in [`EXPORT`]
    /// through JavaScript, with `Function::call`, and through JavaScript
    /// again, and records in [`SEEN`] how each call went.
    fn reenter(store: &mut impl AsStoreMut) {
        let (run, export, ty) = EXPORT.with(|slot| slot.borrow().clone()).unwrap();
        let through_javascript = || {
            let argument = match ty {
                Type::I32 => JsValue::from(3),
                _ => JsValue::from(js_sys::BigInt::from(3)),
            };
            let ran = RAN.get();
            let result = export.call1(&JsValue::UNDEFINED, &argument);
            let error = result.err().map(|error| RuntimeError::from(error).message());
            (error, RAN.get() - ran)
        };
        let first = through_javascript();
        let ran = RAN.get();
        let result = run.call(store, &[argument(ty)]);
        let called = (result.err().map(|error| error.message()), RAN.get() - ran);
        let last = through_javascript();
        SEEN.with(|seen| *seen.borrow_mut() = vec![first, called, last]);
    }

    /// What [`reenter`] sees: the host function JavaScript reaches fails
    /// without running, and the one the call reaches runs.
    fn assert_seen(what: &str) {
        let seen = SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()));
        assert_eq!(seen.len(), 3, "{what}: the host function did not run");
        for (error, ran) in [&seen[0], &seen[2]] {
            let error = error.as_deref().unwrap_or_default();
            assert!(error.contains("uses its store"), "{what}: {error:?}");
            assert_eq!(*ran, 0, "{what}: the host function JavaScript reached ran");
        }
        assert_eq!(seen[1], (None, 1), "{what}: the call");
    }

    /// A host function holds its store while it runs. The calls it makes
    /// run their guests' host functions, but a host function that JavaScript
    /// reaches by calling a guest's export fails without running, of any
    /// kind, and whatever kind the host function that holds the store is.
    /// Once that one returns, the store is free again.
    #[wasm_bindgen_test]
    fn a_host_function_holds_its_store_while_it_runs() {
        let mut store = Store::default();
        let env = FunctionEnv::new(&mut store, ());
        let typed = Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<()>| {
            reenter(&mut env)
        });
        let dynamic = Function::new_with_env(
            &mut store,
            &env,
            FunctionType::new(vec![], vec![]),
            |mut env, _| {
                reenter(&mut env);
                Ok(vec![])
            },
        );
        let module = Module::new(
            &store,
            r#"(module (import "env" "holds" (func $holds)) (func (export "run") call $holds))"#,
        )
        .unwrap();
        let holders = [typed, dynamic].map(|holds| {
            let imports = imports! { "env" => { "holds" => holds } };
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            instance.exports.get_function("run").unwrap().clone()
        });
        for (function, ty) in host_functions(&mut store) {
            let run = guest(&mut store, function, ty);
            let export = run.as_jsvalue(&store).unchecked_into();
            EXPORT.with(|slot| *slot.borrow_mut() = Some((run.clone(), export, ty)));
            for (kind, holder) in ["typed", "dynamic"].iter().zip(&holders) {
                holder.call(&mut store, &[]).unwrap();
                assert_seen(&format!("{ty} in a {kind} host function"));
                assert_eq!(lent(), 0);
                RAN.set(0);
                run.call(&mut store, &[argument(ty)]).unwrap();
                assert_eq!(RAN.get(), 1, "{ty}: the store is free again");
            }
        }
        EXPORT.with(|slot| slot.borrow_mut().take());
    }

    /// A JavaScript exception that unwinds a host function without running
    /// it leaves its hold behind, in the lend of the call it ran in: the call
    /// takes it off with the lend, and the next call runs the host function.
    #[wasm_bindgen_test]
    fn the_hold_an_exception_leaves_behind_goes_with_the_lend() {
        let mut store = Store::default();
        let throws = Function::new_typed(&mut store, |x: i32| -> i32 {
            if x == 0 {
                wasm_bindgen::throw_str("unwound")
            }
            body(x)
        });
        let run = guest(&mut store, throws, Type::I32);
        assert!(run.call(&mut store, &[Value::I32(0)]).is_err());
        assert_eq!(lent(), 0);
        RAN.set(0);
        let results = run.call(&mut store, &[Value::I32(3)]).unwrap();
        assert_eq!(results.to_vec(), vec![Value::I32(3)]);
        assert_eq!(RAN.get(), 1);
    }

    /// A host function of a slice of a guest of `call_async` that JavaScript
    /// resumed holds the store the guest holds, the same way.
    #[cfg(feature = "experimental-async")]
    #[wasm_bindgen_test]
    async fn a_host_function_of_a_resumed_guest_holds_its_store() {
        let mut store = Store::default();
        let (function, ty) = host_functions(&mut store).remove(0);
        let run = guest(&mut store, function, ty);
        let export = run.as_jsvalue(&store).unchecked_into();
        EXPORT.with(|slot| *slot.borrow_mut() = Some((run, export, ty)));
        let wait = Function::new_typed_async(&mut store, async || -> i32 {
            let resolved = js_sys::Promise::resolve(&JsValue::from(3));
            wasm_bindgen_futures::JsFuture::from(resolved)
                .await
                .unwrap();
            3
        });
        let env = FunctionEnv::new(&mut store, ());
        let holds = Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<()>| {
            reenter(&mut env)
        });
        let module = Module::new(
            &store,
            r#"(module
              (import "env" "wait" (func $wait (result i32)))
              (import "env" "holds" (func $holds))
              (func (export "run") (result i32) call $wait call $holds))"#,
        )
        .unwrap();
        let imports = imports! { "env" => { "wait" => wait, "holds" => holds } };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let run = instance.exports.get_function("run").unwrap().clone();
        let store = store.into_async();
        let results = run.call_async(&store, vec![]).await.unwrap();
        assert_eq!(results.to_vec(), vec![Value::I32(3)]);
        assert_seen("in a resumed guest");
        assert_eq!(lent(), 0);
        EXPORT.with(|slot| slot.borrow_mut().take());
    }
}
