//! Context-local JSPI callbacks and continuation handoffs on the owning worker.

use super::{Shared, lock};
use crate::cputime::CpuContext;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    future::{Future, poll_fn},
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
};
use wasm_bindgen::JsValue;
use wasmer::{RuntimeError, js::JspiFuture};
use wasmer_types::StoreId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Call {
    context: u64,
    id: u64,
}
struct Physical {
    context: u64,
    parent: Option<u64>,
}

#[derive(Default)]
pub(super) struct State {
    cpu: Option<CpuContext>,
    // Ready admits a Wasm continuation, whose next microtask still belongs to
    // this id. It ends at the next import or the owning Promise's settlement.
    handoff: Option<Call>,
    calls: BTreeMap<u64, Physical>,
    current_call: BTreeMap<u64, u64>,
    next_call: u64,
    retiring: Option<u64>,
    driver: Option<Waker>,
    waiters: Vec<Waker>,
    drained: bool,
    cancellation: Arc<()>,
}

thread_local! {
    static GROUPS: RefCell<HashMap<StoreId, Weak<Shared>>> = RefCell::new(HashMap::new());
    static CALLBACK: RefCell<Option<(Weak<Shared>, u64)>> = const { RefCell::new(None) };
}

pub(super) struct Registration(StoreId, Weak<Shared>);
impl Drop for Registration {
    fn drop(&mut self) {
        GROUPS.with_borrow_mut(|groups| {
            if groups
                .get(&self.0)
                .is_some_and(|group| group.ptr_eq(&self.1))
            {
                groups.remove(&self.0);
            }
        });
    }
}

pub(super) fn register(
    store: StoreId,
    group: &Arc<Shared>,
    cpu: CpuContext,
    javascript: bool,
) -> Option<Registration> {
    if !javascript {
        return None;
    }
    let weak = Arc::downgrade(group);
    GROUPS.with_borrow_mut(|groups| {
        assert!(
            groups.insert(store, weak.clone()).is_none(),
            "duplicate JSPI context owner"
        );
    });
    lock(&group.state).jspi.cpu = Some(cpu);
    Some(Registration(store, weak))
}

pub(super) fn register_driver(group: &Shared, waker: &Waker) {
    let waker = waker.clone();
    let previous = {
        let mut state = lock(&group.state);
        if state.jspi.cpu.is_some() {
            state.jspi.driver.replace(waker)
        } else {
            Some(waker)
        }
    };
    drop(previous);
}

pub(super) fn changed(group: &Shared) {
    let (driver, waiters) = {
        let mut state = lock(&group.state);
        (
            state.jspi.driver.take(),
            std::mem::take(&mut state.jspi.waiters),
        )
    };
    let mut failures = Vec::new();
    for waker in driver.into_iter().chain(waiters) {
        // Notify every retained driver even if one custom Waker panics. Keep
        // wake and final Drop separate, as in the native invocation scope.
        let wake = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()));
        let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
        failures.extend([wake, retired].into_iter().filter_map(Result::err));
    }
    if !failures.is_empty() {
        let first = failures.remove(0);
        for failure in failures {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failure)));
        }
        std::panic::resume_unwind(first);
    }
}

fn wait(state: &mut super::State, waker: &mut Option<Waker>) {
    if !state
        .jspi
        .waiters
        .iter()
        .any(|old| old.will_wake(waker.as_ref().unwrap()))
    {
        state.jspi.waiters.push(waker.take().unwrap());
    }
}

pub(super) async fn before_activate(group: &Shared, id: u64) {
    poll_fn(|cx| {
        let mut waker = Some(cx.waker().clone());
        let mut state = lock(&group.state);
        if state.jspi.handoff.is_none_or(|owner| owner.context == id) {
            Poll::Ready(())
        } else {
            wait(&mut state, &mut waker);
            Poll::Pending
        }
    })
    .await
}

pub(super) fn may_activate(state: &super::State, id: u64) -> bool {
    state.jspi.handoff.is_none_or(|owner| owner.context == id)
}

pub(super) fn select_retirement(group: &Shared, id: u64) {
    lock(&group.state).jspi.retiring = Some(id);
    changed(group);
}

pub(super) fn root_settled(group: &Shared, id: u64) {
    {
        let mut state = lock(&group.state);
        assert!(!state.jspi.calls.values().any(|call| call.context == id));
        if state.jspi.retiring == Some(id) {
            state.jspi.retiring = None;
        }
    }
    changed(group);
}

pub(super) async fn settle_context(group: &Shared, id: u64) {
    poll_fn(|cx| {
        let mut waker = Some(cx.waker().clone());
        let mut state = lock(&group.state);
        if !state.jspi.calls.values().any(|call| call.context == id) {
            Poll::Ready(())
        } else {
            wait(&mut state, &mut waker);
            Poll::Pending
        }
    })
    .await
}

pub(super) fn drained(group: &Shared) {
    {
        let mut state = lock(&group.state);
        assert!(state.jspi.calls.is_empty());
        state.jspi.drained = true;
    }
    changed(group);
}

pub(crate) async fn settle(group: &Shared) {
    poll_fn(|cx| {
        let mut waker = Some(cx.waker().clone());
        let mut state = lock(&group.state);
        if state.jspi.cpu.is_none() || state.jspi.drained {
            Poll::Ready(())
        } else {
            wait(&mut state, &mut waker);
            Poll::Pending
        }
    })
    .await
}

struct Current(Option<(Weak<Shared>, u64)>);
impl Current {
    fn install(group: &Arc<Shared>, id: u64) -> Self {
        Self(CALLBACK.with_borrow_mut(|current| current.replace((Arc::downgrade(group), id))))
    }
}
impl Drop for Current {
    fn drop(&mut self) {
        CALLBACK.with_borrow_mut(|current| *current = self.0.take());
    }
}

pub(super) fn callback_supported() -> bool {
    CALLBACK.with_borrow(|current| {
        current.as_ref().is_some_and(|(group, id)| {
            group.upgrade().is_some_and(|group| {
                let state = lock(&group.state);
                !state.closed && state.current == *id && state.active == *id
            })
        })
    })
}

#[derive(Debug, thiserror::Error)]
#[error("JSPI context invocation canceled")]
struct Canceled {
    group: Arc<()>,
    id: u64,
}

fn cancellation(group: &Shared, id: u64) -> RuntimeError {
    RuntimeError::host_abort(Box::new(Canceled {
        group: lock(&group.state).jspi.cancellation.clone(),
        id,
    }))
}

pub(super) fn is_cancellation(group: &Shared, id: u64, error: &RuntimeError) -> bool {
    error.downcast_ref::<Canceled>().is_some_and(|error| {
        error.id == id && Arc::ptr_eq(&error.group, &lock(&group.state).jspi.cancellation)
    })
}

enum Admission {
    Waiting,
    Active,
    Canceled,
}
fn admission(group: &Shared, call: Call, waker: &Waker) -> Admission {
    let id = call.context;
    let mut waker = Some(waker.clone());
    let mut state = lock(&group.state);
    if state.current != id || state.jspi.handoff.is_some_and(|owner| owner != call) {
        wait(&mut state, &mut waker);
        return Admission::Waiting;
    }
    if state.jspi.retiring == Some(id) {
        return Admission::Canceled;
    }
    if state.closed || !state.contexts.contains_key(&id) || state.active != id {
        wait(&mut state, &mut waker);
        return Admission::Waiting;
    }
    Admission::Active
}

struct Callback {
    group: Arc<Shared>,
    call: Call,
    cpu: CpuContext,
    system: bool,
    body: Option<JspiFuture>,
    result: Option<Result<JsValue, RuntimeError>>,
    result_order: Option<u64>,
}
impl Callback {
    fn retain_error(&mut self) {
        if let Some(Err(error)) = self.result.take() {
            self.group.retired_error(
                self.call.context,
                self.result_order.take().expect("cached error order"),
                error,
            );
        }
    }

    fn cancel(&mut self) -> Result<JsValue, JsValue> {
        let _current = Current::install(&self.group, self.call.context);
        // Retain the original before dropping a canceled provider future.
        self.retain_error();
        let body = self.body.take();
        self.cpu.run(&mut self.system, || drop(body));
        Err(cancellation(&self.group, self.call.context).into())
    }
}
impl Future for Callback {
    type Output = Result<JsValue, JsValue>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match admission(&this.group, this.call, cx.waker()) {
            Admission::Waiting => return Poll::Pending,
            Admission::Canceled => return Poll::Ready(this.cancel()),
            Admission::Active => (),
        }
        if this.result.is_none() {
            let _current = Current::install(&this.group, this.call.context);
            if let Poll::Ready(result) = this.cpu.run(&mut this.system, || {
                this.body.as_mut().unwrap().as_mut().poll(cx)
            }) {
                if result.is_err() {
                    this.result_order = Some(this.group.error_order());
                    let Err(value) = result else { unreachable!() };
                    let error = RuntimeError::from(value);
                    // Committed exit/exec cannot be caught by a guest or an
                    // enclosing host callback. Restore remains level-local.
                    let retiring = error.downcast_ref::<crate::Exited>().is_some()
                        || error.downcast_ref::<crate::process::ExecNew>().is_some()
                        || error.downcast_ref::<crate::process::Detached>().is_some();
                    this.result = Some(Err(error.clone()));
                    if retiring {
                        this.group
                            .close_with_outcome(Some((this.result_order.unwrap(), error)));
                    }
                } else {
                    this.result = Some(result.map_err(RuntimeError::from));
                }
                let body = this.body.take();
                this.cpu.run(&mut this.system, || drop(body));
            }
        }
        match admission(&this.group, this.call, cx.waker()) {
            Admission::Waiting => Poll::Pending,
            Admission::Canceled => Poll::Ready(this.cancel()),
            Admission::Active => match this.result.take() {
                Some(result) => {
                    let mut state = lock(&this.group.state);
                    assert!(state.jspi.handoff.is_none_or(|owner| owner == this.call));
                    state
                        .jspi
                        .current_call
                        .insert(this.call.context, this.call.id);
                    state.jspi.handoff = Some(this.call);
                    drop(state);
                    Poll::Ready(result.map_err(JsValue::from))
                }
                None => Poll::Pending,
            },
        }
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        let _current = Current::install(&self.group, self.call.context);
        let custody =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.retain_error()));
        let body = self.body.take();
        let retired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.cpu.run(&mut self.system, || drop(body));
        }));
        let mut failures: Vec<_> = [custody, retired]
            .into_iter()
            .filter_map(Result::err)
            .collect();
        if !failures.is_empty() {
            let first = failures.remove(0);
            for failure in failures {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failure)));
            }
            if already_unwinding {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(first)));
            } else {
                std::panic::resume_unwind(first);
            }
        }
    }
}

/// Called synchronously by the Store's JSPI import driver. The group and exact
/// id are captured before the independently driven callback can reach a microtask.
pub(crate) fn callback(store: StoreId, body: JspiFuture) -> JspiFuture {
    let Some(group) = GROUPS.with_borrow(|groups| groups.get(&store).and_then(Weak::upgrade))
    else {
        return body;
    };
    let (call, cpu) = {
        let mut state = lock(&group.state);
        let call = match state.jspi.handoff.take() {
            Some(call) => call,
            None => Call {
                context: state.current,
                id: *state
                    .jspi
                    .current_call
                    .get(&state.current)
                    .expect("JSPI callback requires its physical call owner"),
            },
        };
        assert_eq!(
            call.context, state.current,
            "JSPI continuation changed context owner"
        );
        assert_eq!(
            state.jspi.calls.get(&call.id).unwrap().context,
            call.context
        );
        state.jspi.current_call.insert(call.context, call.id);
        (call, state.jspi.cpu.as_ref().unwrap().clone())
    };
    changed(&group);
    Box::pin(Callback {
        group,
        call,
        cpu,
        system: false,
        body: Some(body),
        result: None,
        result_order: None,
    })
}

struct CallOwner {
    group: Arc<Shared>,
    call: Call,
}
impl wasmer::js::JspiCallOwner for CallOwner {
    fn settled(
        self: Box<Self>,
        outcome: &mut Result<JsValue, JsValue>,
    ) -> Result<(), RuntimeError> {
        if outcome.is_err() {
            // A wasm-bindgen Trap wrapper is consumed by conversion. Rebuild the
            // transport from that same RuntimeError rather than converting twice.
            let Err(value) = outcome else { unreachable!() };
            let error = RuntimeError::from(value.clone());
            let retired = {
                let state = lock(&self.group.state);
                state.closed || !state.contexts.contains_key(&self.call.context)
            };
            if retired {
                // Reuse import error custody while building the own retirement
                // marker. Do not add an ordinary typed transport clone to the
                // abandoned observed Promise and delay its downcast until JS GC.
                let custody = super::PendingError::new(
                    Some((Arc::downgrade(&self.group), self.call.context)),
                    Some(&error),
                );
                *outcome = Err(cancellation(&self.group, self.call.context).into());
                drop(error);
                drop(custody);
            } else {
                *outcome = Err(error.into());
            }
        }
        // Drop retires the exact physical metadata, including if conversion or
        // an original error destructor unwinds before this method returns.
        Ok(())
    }
}

impl Drop for CallOwner {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        {
            let mut state = lock(&self.group.state);
            let physical = state
                .jspi
                .calls
                .remove(&self.call.id)
                .expect("physical JSPI call settles once");
            assert_eq!(physical.context, self.call.context);
            if state.jspi.handoff == Some(self.call) {
                state.jspi.handoff = None;
            }
            if state.jspi.current_call.get(&self.call.context) == Some(&self.call.id) {
                match physical
                    .parent
                    .filter(|parent| state.jspi.calls.contains_key(parent))
                {
                    Some(parent) => {
                        state.jspi.current_call.insert(self.call.context, parent);
                    }
                    None => {
                        state.jspi.current_call.remove(&self.call.context);
                    }
                }
            }
        }

        let notification = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            changed(&self.group);
        }));
        if let Err(failure) = notification {
            if already_unwinding {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failure)));
            } else {
                std::panic::resume_unwind(failure);
            }
        }
    }
}

/// Frozen with the Store callback driver; called before every physical JSPI entry.
pub(crate) fn call_owner(
    store: StoreId,
) -> Result<Option<Box<dyn wasmer::js::JspiCallOwner>>, RuntimeError> {
    let Some(group) = GROUPS.with_borrow(|groups| groups.get(&store).and_then(Weak::upgrade))
    else {
        return Ok(None);
    };
    let call = {
        let mut state = lock(&group.state);
        let context = state.current;
        if state.closed
            || !state.contexts.contains_key(&context)
            || state.jspi.retiring == Some(context)
        {
            drop(state);
            return Err(cancellation(&group, context));
        }
        if state.active != context || state.jspi.handoff.is_some() {
            return Err(RuntimeError::host_abort(Box::new(std::io::Error::other(
                "JSPI guest call entered without continuation custody",
            ))));
        }
        let id = state.jspi.next_call;
        state.jspi.next_call = id
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("JSPI physical call identity exhausted"))?;
        let parent = state.jspi.current_call.get(&context).copied();
        state.jspi.calls.insert(id, Physical { context, parent });
        state.jspi.current_call.insert(context, id);
        Call { context, id }
    };
    Ok(Some(Box::new(CallOwner { group, call })))
}

pub(super) fn is_drained(group: &Shared) -> bool {
    lock(&group.state).jspi.drained
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    use wasm_bindgen::{JsCast, closure::Closure};
    use wasm_bindgen_test::wasm_bindgen_test;
    use wasmer::{
        AsStoreAsync, AsStoreMut, Function, Instance, Module, Store, TypedFunction, imports,
    };

    thread_local! {
        static CANCEL_READY: Cell<bool> = const { Cell::new(false) };
    }

    fn poll_now(future: &mut JspiFuture) -> Poll<Result<JsValue, JsValue>> {
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    fn executor(store: StoreId, body: JspiFuture) -> js_sys::Promise {
        let mut future = callback(store, body);
        let Poll::Ready(result) = poll_now(&mut future) else {
            panic!("this fixture's callbacks complete immediately");
        };
        let promise = match result {
            Ok(value) => js_sys::Promise::resolve(&value),
            Err(error) => js_sys::Promise::reject(&error),
        };
        if !CANCEL_READY.replace(false) {
            return promise;
        }
        let group = GROUPS.with_borrow(|groups| groups[&store].upgrade().unwrap());
        let cancel = Closure::once_into_js(move |value: JsValue| {
            assert_eq!(
                lock(&group.state).jspi.handoff.map(|owner| owner.context),
                Some(0)
            );
            group.close();
            select_retirement(&group, 0);
            let mut other = Box::pin(before_activate(&group, 1));
            assert!(
                other
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            value
        });
        let then = js_sys::Reflect::get(&promise, &JsValue::from_str("then"))
            .unwrap()
            .unchecked_into::<js_sys::Function>();
        then.call1(&promise, &cancel).unwrap().unchecked_into()
    }

    #[wasm_bindgen_test]
    async fn ready_then_cancellation_keeps_the_next_wasm_import_on_the_same_context() {
        let mut store = Store::default();
        let id = store.id();
        wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
        wasmer::js::set_jspi_store_executor(&mut store, executor, call_owner).unwrap();
        let first = Function::new_typed_async(&mut store, || async {
            assert!(super::super::context_operations_supported());
        });
        let calls = Rc::new(Cell::new(0));
        let entered = calls.clone();
        let next = Function::new_typed_async(&mut store, move || {
            entered.set(entered.get() + 1);
            async {}
        });
        let module = Module::new(
            &store,
            r#"(module
          (import "test" "first" (func $first))
          (import "test" "next" (func $next))
          (global $continued (export "continued") (mut i32) (i32.const 0))
          (func (export "run")
            (call $first)
            (block $caught
              (try_table (catch_all $caught) (call $next))
              (global.set $continued (i32.const 1)) (return))
            (global.set $continued (i32.const 2))))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "test" => { "first" => first, "next" => next } },
        )
        .unwrap();
        let run: TypedFunction<(), ()> =
            instance.exports.get_typed_function(&store, "run").unwrap();
        let continued = instance.exports.get_global("continued").unwrap().clone();
        let (_, _, mut sample) = crate::cputime::tests::setup(false);
        let group = Shared::new(wasixv2_syscalls::TaskId(1));
        let _registration = register(id, &group, sample.context(), true).unwrap();
        CANCEL_READY.set(true);
        let store = store.into_async();
        let error = run.call_async(&store).await.unwrap_err();
        assert!(is_cancellation(&group, 0, &error));
        assert_eq!(
            calls.get(),
            0,
            "deleted context must not enter the next import body"
        );
        let mut guard = store.write_lock().await;
        assert_eq!(
            continued.get(&mut guard.as_store_mut()).unwrap_i32(),
            0,
            "host abort must bypass Wasm catch_all"
        );
        drop(guard);
        group.retired_error(0, group.error_order(), error);
        assert!(
            lock(&group.state).original_error.is_none(),
            "internal cancellation is not a guest fault"
        );
        assert_eq!(lock(&group.state).jspi.handoff, None);
    }

    fn cancel_cached_error(store: StoreId, body: JspiFuture) -> js_sys::Promise {
        let mut future = callback(store, body);
        assert!(
            poll_now(&mut future).is_pending(),
            "original error is gated before cancellation"
        );
        let group = GROUPS.with_borrow(|groups| groups[&store].upgrade().unwrap());
        group.close();
        group.retired_error(
            1,
            group.error_order(),
            RuntimeError::new("later sibling error"),
        );
        select_retirement(&group, 0);
        let Poll::Ready(Err(error)) = poll_now(&mut future) else {
            panic!("retirement must reject the cached callback");
        };
        js_sys::Promise::reject(&error)
    }

    #[wasm_bindgen_test]
    async fn cached_original_error_precedes_later_sibling_error_and_cannot_resume_deleted_wasm() {
        #[derive(Debug, thiserror::Error)]
        #[error("original cached callback error")]
        struct Original(Arc<()>);
        let mut store = Store::default();
        let id = store.id();
        wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
        wasmer::js::set_jspi_store_executor(&mut store, cancel_cached_error, call_owner).unwrap();
        let (_, _, mut sample) = crate::cputime::tests::setup(false);
        let group = Shared::new(wasixv2_syscalls::TaskId(1));
        let _registration = register(id, &group, sample.context(), true).unwrap();
        let payload = Arc::new(());
        let captured = payload.clone();
        let owner = group.clone();
        let fail = Function::new_typed_async(&mut store, move || {
            // Model a nested host handoff before this callback's original error
            // can be returned. The error must be cached without resolving JSPI.
            {
                let mut state = lock(&owner.state);
                state.active = 1;
                state.contexts.insert(0, super::super::Status::Suspended);
                state.contexts.insert(1, super::super::Status::Active);
            }
            let error = RuntimeError::user(Box::new(Original(captured.clone())));
            async move { Err::<(), _>(error) }
        });
        let module = Module::new(
            &store,
            r#"(module
          (import "test" "fail" (func $fail))
          (global $continued (export "continued") (mut i32) (i32.const 0))
          (func (export "run")
            (block $caught
              (try_table (catch_all $caught) (call $fail))
              (global.set $continued (i32.const 1)) (return))
            (global.set $continued (i32.const 2))))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &imports! { "test" => { "fail" => fail } },
        )
        .unwrap();
        let run: TypedFunction<(), ()> =
            instance.exports.get_typed_function(&store, "run").unwrap();
        let continued = instance.exports.get_global("continued").unwrap().clone();
        let store = store.into_async();
        let cancellation = run.call_async(&store).await.unwrap_err();
        assert!(is_cancellation(&group, 0, &cancellation));
        group.retired_error(0, group.error_order(), cancellation);
        let original = group
            .resolve(Ok(()))
            .unwrap_err()
            .downcast::<Original>()
            .unwrap();
        assert!(Arc::ptr_eq(&payload, &original.0));
        let mut guard = store.write_lock().await;
        assert_eq!(continued.get(&mut guard.as_store_mut()).unwrap_i32(), 0);
    }

    thread_local! {
        static ABANDONED_ERROR: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    }

    struct RecordOwner {
        inner: Box<dyn wasmer::js::JspiCallOwner>,
        id: u64,
    }
    impl wasmer::js::JspiCallOwner for RecordOwner {
        fn settled(
            self: Box<Self>,
            outcome: &mut Result<JsValue, JsValue>,
        ) -> Result<(), RuntimeError> {
            if self.id == 1
                && let Err(value) = outcome
            {
                ABANDONED_ERROR.with_borrow_mut(|error| *error = Some(value.clone()));
            }
            self.inner.settled(outcome)
        }
    }
    fn record_owner(
        store: StoreId,
    ) -> Result<Option<Box<dyn wasmer::js::JspiCallOwner>>, RuntimeError> {
        let inner = call_owner(store)?.unwrap();
        let group = GROUPS.with_borrow(|groups| groups[&store].upgrade().unwrap());
        let id = lock(&group.state).jspi.current_call[&0];
        Ok(Some(Box::new(RecordOwner { inner, id })))
    }
    fn cancel_after_leaf_ready(store: StoreId, body: JspiFuture) -> js_sys::Promise {
        let group = GROUPS.with_borrow(|groups| groups[&store].upgrade().unwrap());
        let id = lock(&group.state).jspi.current_call[&0];
        let promise = wasm_bindgen_futures::future_to_promise(callback(store, body));
        if id != 1 {
            return promise;
        }
        let cancel = Closure::once_into_js(move |value: JsValue| {
            assert_eq!(
                lock(&group.state).jspi.handoff,
                Some(Call { context: 0, id: 1 })
            );
            group.close();
            select_retirement(&group, 0);
            value
        });
        let then = js_sys::Reflect::get(&promise, &JsValue::from_str("then"))
            .unwrap()
            .unchecked_into::<js_sys::Function>();
        then.call1(&promise, &cancel).unwrap().unchecked_into()
    }

    #[wasm_bindgen_test]
    async fn nested_physical_rejection_survives_cancellation_of_its_rust_waiter() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use wasmer::{AsyncFunctionEnvMut, FunctionEnv};
        #[derive(Debug, thiserror::Error)]
        #[error("late original foreign host-abort")]
        struct Original(Arc<()>);
        for kind in [0, 1, 2, 3, 4, 5] {
            let typed = kind == 1 || kind == 2;
            let host_abort = kind == 2;
            ABANDONED_ERROR.with_borrow_mut(|error| *error = None);
            let payload = Arc::new(());
            let returned_payload = payload.clone();
            let mut store = Store::default();
            let id = store.id();
            wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
            wasmer::js::set_jspi_store_executor(&mut store, cancel_after_leaf_ready, record_owner)
                .unwrap();
            let env = FunctionEnv::new(&mut store, None::<Function>);
            let observed = Arc::new(AtomicBool::new(false));
            let body_observed = observed.clone();
            let nested = Function::new_typed_with_env_async(
                &mut store,
                &env,
                move |env: AsyncFunctionEnvMut<Option<Function>>| {
                    let observed = body_observed.clone();
                    async move {
                        let leaf = env.read().await.data().as_ref().unwrap().clone();
                        let result = leaf.call_async(&env, vec![]).await;
                        observed.store(true, Ordering::SeqCst);
                        result.map(drop)
                    }
                },
            );
            let ready = Function::new_typed_async(&mut store, || async {});
            let late_env = FunctionEnv::new(&mut store, ());
            let late = Function::new_with_env(
                &mut store,
                &late_env,
                wasmer::FunctionType::new([], []),
                move |_, _| {
                    if kind >= 3 {
                        let source = match kind {
                            3 => "return { toString() { throw new Error('diagnostic failed'); } };",
                            4 => {
                                "return Object.create(Object.defineProperty({}, 'constructor', { get() { throw new Error('constructor failed'); } }));"
                            }
                            _ => {
                                "return new Proxy({}, { getPrototypeOf() { throw new Error('prototype failed'); }, get() { throw new Error('property failed'); } });"
                            }
                        };
                        let value = js_sys::Function::new_no_args(source)
                            .call0(&JsValue::UNDEFINED)
                            .unwrap();
                        Err(RuntimeError::from(value))
                    } else if !typed {
                        Ok(vec![])
                    } else if host_abort {
                        Err(RuntimeError::host_abort(Box::new(Original(
                            returned_payload.clone(),
                        ))))
                    } else {
                        Err(RuntimeError::user(Box::new(Original(
                            returned_payload.clone(),
                        ))))
                    }
                },
            );
            let module = Module::new(
                &store,
                r#"(module
          (import "test" "nested" (func $nested))
          (import "test" "ready" (func $ready))
          (import "test" "late" (func $late))
          (func (export "leaf") (call $ready) (call $late) unreachable)
          (func (export "run") (call $nested)))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "test" => { "nested" => nested, "ready" => ready, "late" => late } },
            )
            .unwrap();
            *env.as_mut(&mut store) = Some(instance.exports.get_function("leaf").unwrap().clone());
            let run: TypedFunction<(), ()> =
                instance.exports.get_typed_function(&store, "run").unwrap();
            let (_, _, mut sample) = crate::cputime::tests::setup(false);
            let group = Shared::new(wasixv2_syscalls::TaskId(1));
            let _registration = register(id, &group, sample.context(), true).unwrap();
            let error = run.call_async(&store.into_async()).await.unwrap_err();
            assert!(is_cancellation(&group, 0, &error));
            settle_context(&group, 0).await;
            assert!(
                !observed.load(Ordering::SeqCst),
                "outer callback must drop its nested Rust waiter"
            );
            let original = group.resolve(Ok(())).unwrap_err();
            if typed {
                assert_eq!(original.is_host_abort(), host_abort);
                assert!(
                    !is_cancellation(&group, 0, &original),
                    "foreign original must survive"
                );
                if host_abort {
                    // Host-abort deliberately supports repeated non-consuming
                    // rethrows; its raw Promise still owns the same payload.
                    assert!(Arc::ptr_eq(
                        &payload,
                        &original.downcast_ref::<Original>().unwrap().0
                    ));
                } else {
                    let original = original.downcast::<Original>().unwrap();
                    assert!(Arc::ptr_eq(&payload, &original.0));
                }
                ABANDONED_ERROR.with_borrow_mut(|error| drop(error.take()));
            } else {
                let original: JsValue = original.into();
                ABANDONED_ERROR.with_borrow_mut(|error| {
                    assert!(js_sys::Object::is(
                        &original,
                        &error.take().expect("physical leaf rejection")
                    ));
                });
            }
            assert!(lock(&group.state).jspi.calls.is_empty());
        }
    }

    struct NoSpawn;
    impl crate::process::Spawn for NoSpawn {
        fn spawn(&self, _: wasixv2_syscalls::TaskId, _: crate::Image) {
            unreachable!("the test starts no image")
        }
        fn fork(&self, _: wasixv2_syscalls::TaskId, _: crate::asyncify::ForkImage) {
            unreachable!("the test forks nothing")
        }
        fn take_over(
            &self,
            _: wasixv2_syscalls::TaskId,
            _: crate::Image,
            _: Arc<crate::process::Handoff>,
        ) {
            unreachable!("the test takes over no image")
        }
    }

    fn drive_callback(store: StoreId, body: JspiFuture) -> js_sys::Promise {
        wasm_bindgen_futures::future_to_promise(callback(store, body))
    }

    #[wasm_bindgen_test]
    async fn driver_preserves_committed_exit_and_exec_through_nested_host_catch() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use wasmer::{AsyncFunctionEnvMut, FunctionEnv};
        for exec in [false, true] {
            let mut store = Store::default();
            wasmer::js::set_async_mode(&mut store, wasmer::js::AsyncMode::Jspi).unwrap();
            wasmer::js::set_jspi_store_executor(&mut store, drive_callback, call_owner).unwrap();
            let calls = FunctionEnv::new(&mut store, None::<Function>);
            let caught = Arc::new(AtomicBool::new(false));
            let body_caught = caught.clone();
            let nested = Function::new_typed_with_env_async(
                &mut store,
                &calls,
                move |env: AsyncFunctionEnvMut<Option<Function>>| {
                    let caught = body_caught.clone();
                    async move {
                        let leaf = env.read().await.data().as_ref().unwrap().clone();
                        let _ = leaf.call_async(&env, vec![]).await;
                        caught.store(true, Ordering::SeqCst);
                    }
                },
            );
            let control = Function::new_typed_async(&mut store, move || async move {
                let error = if exec {
                    RuntimeError::user(Box::new(crate::process::ExecNew(
                        wasixv2_syscalls::Image::copied(vec![17, 23]),
                    )))
                } else {
                    RuntimeError::user(Box::new(crate::Exited(37)))
                };
                Err::<(), _>(error)
            });
            let module = Module::new(
                &store,
                r#"(module
                (import "test" "nested" (func $nested))
                (import "test" "control" (func $control))
                (memory (export "memory") 1)
                (func (export "leaf") (call $control))
                (func (export "run") (call $nested)))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &imports! { "test" => { "nested" => nested, "control" => control } },
            )
            .unwrap();
            *calls.as_mut(&mut store) =
                Some(instance.exports.get_function("leaf").unwrap().clone());
            let run: TypedFunction<(), ()> =
                instance.exports.get_typed_function(&store, "run").unwrap();
            let host = Arc::new(crate::cputime::tests::Clocks::default());
            let cleanup = wasixv2_host::LoaderCleanup::new(host.clone());
            let mut data = crate::ExecEnv::new(
                Arc::new(crate::process::tests::Recorder::default()),
                wasixv2_syscalls::TaskId(1),
                crate::process::ProcExports::new(Arc::new(NoSpawn), Arc::default()),
                host,
            );
            data.cleanup = Some(cleanup);
            let env = FunctionEnv::new(&mut store, data);
            let store = store.into_async();
            let entry = crate::Entry::new(&env, &store);
            let invocation_store = store.store();
            let mut attachments = None;
            let result = super::super::drive(&entry, &mut attachments, async move {
                run.call_async(&invocation_store).await
            })
            .await;
            let error = attachments
                .as_mut()
                .unwrap()
                .finish(result)
                .await
                .unwrap_err();
            assert!(
                !caught.load(Ordering::SeqCst),
                "committed control must bypass enclosing host catch"
            );
            if exec {
                assert_eq!(
                    error
                        .downcast::<crate::process::ExecNew>()
                        .unwrap()
                        .0
                        .bytes(),
                    [17, 23]
                );
            } else {
                assert_eq!(error.downcast::<crate::Exited>().unwrap().0, 37);
            }
        }
    }
}
