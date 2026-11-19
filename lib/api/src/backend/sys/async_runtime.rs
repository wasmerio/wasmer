#[cfg(feature = "experimental-host-interrupt")]
use std::{
    collections::HashMap,
    sync::{LazyLock, atomic::AtomicU64},
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    future::Future,
    marker::PhantomData,
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    ptr,
    rc::Rc,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder, stack::DefaultStack};
#[cfg(feature = "experimental-host-interrupt")]
use dashmap::DashMap;

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLock,
    LocalRwLockWriteGuard, RuntimeError, Store, StoreAsync, StoreContext, StoreInner, StoreMut,
    StoreRef, Value,
};
use wasmer_types::StoreId;
use wasmer_vm::ActivationState;

pub(crate) type HostFuture =
    Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;
type CallResult = Result<Box<[Value]>, RuntimeError>;

pub(crate) fn call_function_async(
    function: SysFunction,
    store: StoreAsync,
    params: Vec<Value>,
) -> AsyncCallFuture {
    AsyncCallFuture::new(function, store, params)
}

/// Why a coroutine suspended. See [`CoroutineContext::block_on_future`].
enum AsyncYield {
    /// For the poller to drive a host future the coroutine could not poll
    /// itself. The store is still installed.
    Unpolled(HostFuture),
    /// For a host future the coroutine polled with the poller's task context
    /// and found pending. The coroutine has given the store up.
    Pending(HostFuture),
    /// For the store: the host future the coroutine polled completed, or
    /// panicked, but its poll left the store locked. The coroutine has given
    /// the store up, and is to be resumed with this once it is back.
    StoreTaken(AsyncResume),
}

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
    /// The host future the coroutine polled itself panicked with this, and
    /// the panic goes on from where the poll was made.
    HostFuturePanicked(Box<dyn Any + Send>),
}

#[cfg(feature = "experimental-host-interrupt")]
type AsyncCallFutureId = u64;

#[cfg(feature = "experimental-host-interrupt")]
static NEXT_ASYNC_CALL_FUTURE_ID: AtomicU64 = AtomicU64::new(0);

/// The wakers of the calls that are in progress, by store, for
/// [`notify_pending_futures_of_interrupt`]: a host interrupt is the only
/// thing that wakes a call from outside. Everything else a call waits for,
/// the store's lock or a host future, has the waker itself.
#[cfg(feature = "experimental-host-interrupt")]
static ASYNC_CALL_FUTURE_WAKERS: LazyLock<DashMap<StoreId, HashMap<AsyncCallFutureId, Waker>>> =
    LazyLock::new(DashMap::new);

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    #[cfg(feature = "experimental-host-interrupt")]
    id: AsyncCallFutureId,

    coroutine: Option<Coroutine<AsyncResume, AsyncYield, CallResult>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<CallResult>,

    // Store handle we can use to lock the store down
    store: StoreAsync,

    activation: Activation,
}

/// How many coroutine stacks a thread keeps for its next `call_async`s.
const STACK_POOL_CAPACITY: usize = 16;

thread_local! {
    /// Stacks of `call_async` coroutines that ran to completion on this
    /// thread, for its next calls. A stack is a 1 MiB mapping with a guard
    /// page; mapping and unmapping one costs several times what the rest of a
    /// call does.
    ///
    /// This is not `wasmer_vm`'s cache of Wasm stacks: that one keeps a
    /// single stack per thread, which the Wasm frames of a call already take,
    /// sends the rest through a queue shared by all threads, and discards
    /// stacks smaller than the configured Wasm stack size.
    static STACK_POOL: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
}

/// A stack for a new coroutine: a pooled one if there is any.
fn take_stack() -> DefaultStack {
    // `try_with`: a call can be made while the thread's locals are destroyed.
    STACK_POOL
        .try_with(|pool| pool.borrow_mut().pop())
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Keeps the stack of a coroutine that returned for the thread's next call,
/// or unmaps it if the thread has enough of them.
///
/// Only a coroutine whose function returned gives its stack back. One that
/// was unwound, by a host panic or because its call was dropped while
/// suspended, is dropped with its stack, which is unmapped as before. Those
/// exits are rare and cost far more than a mapping, so reuse is kept to the
/// one exit after which every frame on the stack has returned.
fn give_stack_back(stack: DefaultStack) {
    let _ = STACK_POOL.try_with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < STACK_POOL_CAPACITY {
            pool.push(stack);
        }
    });
}

thread_local! {
    /// The `call_async` coroutine this thread is running on, if any.
    static CURRENT_COROUTINE: Cell<*const CoroutineContext> = const { Cell::new(ptr::null()) };
    /// The task context of the `AsyncCallFuture::poll` that resumed the
    /// coroutine this thread is running on, for as long as the thread stays
    /// on the coroutine. Null while a coroutine is unwound.
    static POLL_CONTEXT: Cell<*mut Context<'static>> = const { Cell::new(ptr::null_mut()) };
}

/// The thread-local state that belongs to one `call_async`: kept here while
/// its coroutine is suspended, and exchanged with the thread's by
/// [`AsyncCallFuture::switch_to_coroutine`], the only place where the thread
/// enters or leaves the coroutine.
///
/// Suspended calls can be resumed in any order, so none of this may be left
/// on the thread while the coroutine is not running: another call would run
/// with it, or put its own on top and have it popped out of order.
struct Activation {
    /// The trap handler the signal handler consults and the interrupt
    /// registry's active stores.
    vm: ActivationState,
    /// [`CURRENT_COROUTINE`].
    coroutine: *const CoroutineContext,
}

impl Activation {
    /// # Safety
    /// See [`ActivationState::swap`].
    unsafe fn swap(&mut self) {
        unsafe { self.vm.swap() };
        CURRENT_COROUTINE.with(|cell| self.coroutine = cell.replace(self.coroutine));
    }
}

// We can't use any of the existing AsStoreMut types here, since we keep
// changing the store context underneath us while the coroutine yields.
// To work around it, we use this dummy struct, which just grabs the store
// from the store context. Since we always have a store context installed
// when resuming the coroutine, this is safe in that it can access the store
// through the store context. HOWEVER, references returned from this struct
// CAN NOT BE HELD ACROSS A YIELD POINT. We don't do this anywhere in the
// `Function::call` code.
struct AsyncCallStoreMut {
    store_id: StoreId,
}

impl AsStoreRef for AsyncCallStoreMut {
    fn as_store_ref(&self) -> StoreRef<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreRef {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_ref()
                    .unwrap(),
            }
        }
    }
}

impl AsStoreMut for AsyncCallStoreMut {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreMut {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_mut()
                    .unwrap(),
            }
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            &mut StoreContext::get_current_transient(self.store_id)
                .as_mut()
                .unwrap()
                .objects
        }
    }
}

impl AsyncCallFuture {
    pub(crate) fn new(function: SysFunction, store: StoreAsync, params: Vec<Value>) -> Self {
        let store_id = store.id;
        let coroutine = Coroutine::with_stack(
            take_stack(),
            move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                assert!(matches!(resume, AsyncResume::Start));

                let ctx_state = CoroutineContext { yielder, store_id };
                CURRENT_COROUTINE.with(|cell| cell.set(&ctx_state));
                let mut store_mut = AsyncCallStoreMut { store_id };
                function.call(&mut store_mut, &params)
            },
        );

        Self {
            #[cfg(feature = "experimental-host-interrupt")]
            id: NEXT_ASYNC_CALL_FUTURE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            coroutine: Some(coroutine),
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            store,
            activation: Activation {
                vm: ActivationState::new(),
                coroutine: ptr::null(),
            },
        }
    }

    /// Runs the coroutine until it suspends or finishes, on behalf of a poll
    /// with the task context `cx`; or, given no `resume`, unwinds a suspended
    /// coroutine (see `Drop`).
    ///
    /// The coroutine's [`Activation`] is on the thread for exactly as long as
    /// the thread is on the coroutine's stack, and is taken back however the
    /// thread leaves it: by suspending, by returning, or by a host panic
    /// propagating out of `resume`.
    fn switch_to_coroutine(
        &mut self,
        resume: Option<(AsyncResume, &mut Context<'_>)>,
    ) -> Option<CoroutineResult<AsyncYield, CallResult>> {
        struct SwitchBack<'a> {
            activation: &'a mut Activation,
            /// [`POLL_CONTEXT`] of the stack the thread switched from.
            poll_context: *mut Context<'static>,
            suspended: bool,
        }
        impl Drop for SwitchBack<'_> {
            fn drop(&mut self) {
                // Safety: the second half of the pair below; the thread is
                // back on the stack it switched from.
                unsafe { self.activation.swap() };
                POLL_CONTEXT.with(|cell| cell.set(self.poll_context));
                if !self.suspended {
                    // The coroutine returned or was unwound: nothing may be
                    // left pointing into a stack that is gone.
                    self.activation.coroutine = ptr::null();
                    debug_assert!(self.activation.vm.is_empty());
                }
            }
        }

        // Safety: host code is about to switch to the coroutine that owns the
        // activation, on the thread that runs it; `SwitchBack` swaps again as
        // soon as control is back, also when it comes back by unwinding. The
        // state is not used again once the coroutine is gone.
        let (resume_arg, cx) = resume.unzip();
        let poll_context = cx.map_or(ptr::null_mut(), |cx| {
            ptr::from_mut(cx).cast::<Context<'static>>()
        });
        unsafe { self.activation.swap() };
        let mut switch_back = SwitchBack {
            activation: &mut self.activation,
            poll_context: POLL_CONTEXT.with(|cell| cell.replace(poll_context)),
            suspended: false,
        };
        let result = match resume_arg {
            Some(resume_arg) => Some(self.coroutine.as_mut().unwrap().resume(resume_arg)),
            None => {
                self.coroutine = None;
                None
            }
        };
        switch_back.suspended = matches!(result, Some(CoroutineResult::Yield(_)));
        drop(switch_back);
        result
    }

    #[cfg(not(feature = "experimental-host-interrupt"))]
    fn remove_from_wakers_list(&self) {}

    #[cfg(feature = "experimental-host-interrupt")]
    fn remove_from_wakers_list(&self) {
        let mut wakers_entry = match ASYNC_CALL_FUTURE_WAKERS.entry(self.store.id) {
            dashmap::Entry::Occupied(o) => o,
            dashmap::Entry::Vacant(v) => return,
        };

        let mut waker_map_ref = wakers_entry.get_mut();
        waker_map_ref.remove(&self.id);
        if waker_map_ref.is_empty() {
            wakers_entry.remove();
        }
    }
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Whether the coroutine polled the pending host future itself, in
        // this poll, so that the future has this poll's waker already.
        let mut polled = false;
        loop {
            #[cfg(feature = "experimental-host-interrupt")]
            let store_id = self.store.id;

            #[cfg(feature = "experimental-host-interrupt")]
            {
                if super::vm::interrupt_registry::is_interrupted(store_id)
                    || self.activation.vm.is_interrupted(store_id)
                {
                    self.remove_from_wakers_list();
                    return Poll::Ready(Err(super::vm::Trap::lib(
                        super::vm::TrapCode::HostInterrupt,
                    )
                    .into()));
                }
            }

            if polled {
                return Poll::Pending;
            }
            if let Some(future) = self.pending_future.as_mut() {
                match future.as_mut().poll(cx) {
                    Poll::Ready(result) => {
                        self.pending_future = None;
                        self.next_resume = Some(AsyncResume::HostFutureReady(result));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // If we're ready, return early
            if self.coroutine.is_none() {
                self.remove_from_wakers_list();
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            #[cfg(feature = "experimental-host-interrupt")]
            {
                let mut wakers_entry = ASYNC_CALL_FUTURE_WAKERS.entry(store_id).or_default();
                wakers_entry.insert(self.id, cx.waker().clone());
            }

            // Acquiring a store lock should be the last step before resuming
            // the coroutine, to minimize the time we hold the lock.
            //
            // A store that is free is locked on the spot. It cannot be the
            // store this thread is running, which is locked, so there is
            // nothing for `install_store_context` to check. A call that is
            // already waiting for the store leaves it to the future that
            // holds its place among the waiters.
            let free_store = match self.pending_store_install {
                None => self.store.inner.try_write(),
                Some(_) => None,
            };
            let store_context_guard = match free_store {
                Some(store_guard) => StoreContext::install_async(store_guard),
                None => {
                    let this = &mut *self;
                    let store = &this.store;
                    let install = this.pending_store_install.get_or_insert_with(|| {
                        Box::pin(install_store_context(StoreAsync {
                            id: store.id,
                            inner: store.inner.clone(),
                        }))
                    });
                    match install.as_mut().poll(cx) {
                        Poll::Ready(guard) => {
                            this.pending_store_install = None;
                            guard
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
            };

            let resume_arg = self.next_resume.take().expect("no resume arg available");
            // Whether the coroutine gave the store up itself.
            let mut store_given_up = false;
            match self.switch_to_coroutine(Some((resume_arg, cx))) {
                Some(CoroutineResult::Yield(AsyncYield::Unpolled(future))) => {
                    self.pending_future = Some(future);
                }
                Some(CoroutineResult::Yield(AsyncYield::Pending(future))) => {
                    self.pending_future = Some(future);
                    (store_given_up, polled) = (true, true);
                }
                Some(CoroutineResult::Yield(AsyncYield::StoreTaken(resume))) => {
                    self.next_resume = Some(resume);
                    store_given_up = true;
                }
                Some(CoroutineResult::Return(result)) => {
                    give_stack_back(self.coroutine.take().unwrap().into_stack());
                    self.result = Some(result);
                }
                None => unreachable!("resumed with an argument"),
            }

            if store_given_up {
                store_context_guard.entry_was_lent();
            } else {
                // The coroutine has no store context entry of its own: a
                // store it enters synchronously stays on top until that call
                // returns, and cannot suspend in between (see
                // `block_on_host_future`). So the entry installed above is on
                // top again.
                debug_assert!(
                    StoreContext::top_is_async(),
                    "a call_async coroutine left a store context entry behind"
                );
                // Uninstall the store context to unlock the store after the
                // coroutine yields or returns.
                drop(store_context_guard);
            }
        }
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        self.remove_from_wakers_list();
        // A call dropped while suspended is cancelled by unwinding its
        // coroutine: host frames run their destructors, guest frames are
        // discarded without running. The unwind happens on the coroutine, so
        // it needs the coroutine's state like any other resumption. The store
        // is not locked for it and nothing on the way may use it. The pending
        // host future is dropped afterwards, with the other fields.
        if self
            .coroutine
            .as_ref()
            .is_some_and(|c| c.started() && !c.done())
        {
            self.switch_to_coroutine(None);
        }
    }
}

async fn install_store_context(store: StoreAsync) -> ForcedStoreInstallGuard {
    // Looks at the active entry without borrowing it: a borrow taken here
    // would still be taken after the panic below is caught.
    match crate::StoreContext::is_active(store.id) {
        false => {
            // We always need to acquire a new write lock on the store.
            let store_guard = store.inner.write().await;
            unsafe { crate::StoreContext::install_async(store_guard) }
        }
        true => {
            // If we're already in a store context, it is unsafe to reuse
            // the existing store ref since it'll also be accessible from
            // the imported function that tried to poll us, which is a
            // double mutable borrow.
            // Note to people who discover this code: this *would* be safe
            // if we had a separate variation of call_async that just
            // used the existing coroutine context instead of spawning a
            // new coroutine. However, the current call_async always spawns
            // a new coroutine, so we can't allow this; every coroutine
            // needs to own its write lock on the store to make sure there
            // are no overlapping mutable borrows. If this is something
            // you're interested in, feel free to open a GitHub issue outlining
            // your use-case.
            panic!(
                "Function::call_async futures cannot be polled recursively \
                from within another imported function. If you need to await \
                a recursive call_async, consider spawning the future into \
                your async runtime and awaiting the resulting task; \
                e.g. tokio::task::spawn(func.call_async(...)).await"
            );
        }
    }
}

pub enum AsyncRuntimeError {
    YieldOutsideAsyncContext,
    RuntimeError(RuntimeError),
}

pub(crate) fn block_on_host_future(
    future: HostFuture,
    eager: bool,
) -> Result<Vec<Value>, AsyncRuntimeError> {
    match CoroutineContext::get_current() {
        None => {
            // If there is no async context or we haven't entered it,
            // we can still directly run a future that doesn't block
            // inline.
            run_immediate(future)
        }
        // A store entered synchronously from inside a coroutine (another
        // store, called by a host function) is a sync context too.
        Some(_) if !StoreContext::top_is_async() => run_immediate(future),
        Some(context) => unsafe { context.as_ref().expect("valid context pointer") }
            .block_on_future(future, eager)
            .map_err(AsyncRuntimeError::RuntimeError),
    }
}

// This function will only *wake* pending futures belonging to the given store.
// It is expected that the interrupt happens before this function is called, so
// that the futures can catch and react to `interrupt_registry::is_interrupted`
// correctly.
#[cfg(feature = "experimental-host-interrupt")]
pub(crate) fn notify_pending_futures_of_interrupt(store_id: StoreId) {
    let dashmap::Entry::Occupied(entry) = ASYNC_CALL_FUTURE_WAKERS.entry(store_id) else {
        return;
    };

    for waker in entry.get().values() {
        waker.wake_by_ref();
    }
}

/// What host code needs to suspend the coroutine it is running on. It lives
/// on the coroutine's own stack.
struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
    /// The store the call was made on.
    store_id: StoreId,
}

/// A coroutine's first poll of a host future, between lending the store out
/// and ending the loan. See [`CoroutineContext::block_on_future`].
struct FirstPoll {
    coroutine: *const CoroutineContext,
    /// The store's lock, while the store is lent out.
    store: Option<LocalRwLock<Box<StoreInner>>>,
}

impl FirstPoll {
    /// Makes the coroutine current again and, if asked to, locks the store
    /// and installs it. Returns whether the store is installed.
    fn end(&mut self, take_store_back: bool) -> bool {
        CURRENT_COROUTINE.with(|cell| cell.set(self.coroutine));
        match self.store.take() {
            Some(store) if take_store_back => StoreContext::return_async(&store),
            _ => false,
        }
    }
}

impl Drop for FirstPoll {
    fn drop(&mut self) {
        // Something between the loan and its end panicked that
        // `block_on_future` does not catch. The frames the panic unwinds
        // through expect the store where a host function leaves it.
        if self.store.is_some() {
            self.end(true);
        }
    }
}

impl CoroutineContext {
    fn get_current() -> Option<*const Self> {
        let current = CURRENT_COROUTINE.with(|cell| cell.get());
        (!current.is_null()).then_some(current)
    }

    /// Waits for a host future on behalf of the guest, which called an async
    /// import.
    ///
    /// The future of an eager host function (see [`crate::Function::eager`])
    /// is polled once right here, on the coroutine, and the coroutine
    /// suspends only if it is pending; that of any other host function is
    /// handed to the poller unpolled. A future polled here has to find what
    /// it finds when the poller polls it:
    ///
    /// - the store unlocked and not installed, as it is while the coroutine
    ///   is suspended, so that the future can lock it. The coroutine lends it
    ///   out for the poll, which is possible exactly when it could suspend:
    ///   when no host frame of the call holds a borrow of the store;
    /// - no current coroutine. Guest code the future calls into must not be
    ///   able to suspend this coroutine from inside the future's poll;
    /// - the poller's task context.
    ///
    /// Afterwards the coroutine drops the future if it is ready, as the
    /// poller would, and takes the store back. If it cannot, because the
    /// future locked it and something else still holds the lock, it suspends
    /// and the poller waits for the store, as it does before every
    /// resumption. A future that panics is dropped and the store taken back
    /// the same way before the panic goes on, so that a host frame further
    /// up that catches it finds the store where it left it.
    ///
    /// Where this poll differs from the poller's:
    ///
    /// - It runs on the coroutine's stack, a fixed 1 MiB that neither
    ///   `wasmer_vm::set_stack_size` nor the Wasm stack size changes. A poll
    ///   that needs more stack than is left there overflows into the guard
    ///   page, and the process is killed (SIGBUS); polled by the poller, the
    ///   same future would run on the poller's stack.
    /// - Hiding the coroutine affects this poll only. If the call was itself
    ///   polled from a host function of an outer `call_async`, and the
    ///   future enters the outer call's store through `Store::with_current`
    ///   and reaches a pending async import there, that import fails with
    ///   `YieldOutsideAsyncContext` during this poll. If the future is
    ///   pending here and does the same in a later poll, made by this call's
    ///   poller on the outer call's coroutine, the import suspends the outer
    ///   call.
    fn block_on_future(&self, future: HostFuture, eager: bool) -> Result<Vec<Value>, RuntimeError> {
        let cx = POLL_CONTEXT.with(|cell| cell.get());
        let lent = if cx.is_null() {
            // The coroutine is being unwound.
            None
        } else if !eager {
            // Only an eager host function is polled here. The future of any
            // other is polled by the poller, on the executor's stack.
            None
        } else {
            StoreContext::lend_async(self.store_id)
        };
        let yielded = match lent {
            None => AsyncYield::Unpolled(future),
            Some(store) => {
                let mut first_poll = FirstPoll {
                    coroutine: CURRENT_COROUTINE.with(|cell| cell.replace(ptr::null())),
                    store: Some(store),
                };
                // Declared after `first_poll`, so that the future, and
                // whatever it holds, is dropped before the store is taken
                // back however this ends.
                let mut future = future;
                // Safety: this is the context of the poll that resumed the
                // coroutine. That poll is waiting for the coroutine to switch
                // back and does not use the context meanwhile.
                let poll =
                    panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(unsafe {
                        &mut *cx
                    })));
                let resume = match poll {
                    Ok(Poll::Pending) => {
                        first_poll.end(false);
                        return self.suspend(AsyncYield::Pending(future));
                    }
                    Ok(Poll::Ready(result)) => AsyncResume::HostFutureReady(result),
                    Err(payload) => AsyncResume::HostFuturePanicked(payload),
                };
                // A future that is done, or panicked, is dropped before the
                // store is taken back, as the poller drops it: nothing it
                // still owns may keep the store locked. A panic in its
                // destructor goes on as one in its poll does, once the store
                // is back; after a panic in its poll, that one goes on.
                let dropped = panic::catch_unwind(AssertUnwindSafe(move || drop(future)));
                let resume = match (resume, dropped) {
                    (resume, Ok(())) => resume,
                    (resume @ AsyncResume::HostFuturePanicked(_), Err(_)) => resume,
                    (_, Err(payload)) => AsyncResume::HostFuturePanicked(payload),
                };
                if first_poll.end(true) {
                    return Self::resumed(resume);
                }
                AsyncYield::StoreTaken(resume)
            }
        };

        self.suspend(yielded)
    }

    /// Suspends the coroutine, and returns what the host future it waits for
    /// returned once it is resumed.
    fn suspend(&self, yielded: AsyncYield) -> Result<Vec<Value>, RuntimeError> {
        // The poller takes this coroutine's state off the thread and puts it
        // back before resuming us; see `AsyncCallFuture::switch_to_coroutine`.
        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        Self::resumed(yielder.suspend(yielded))
    }

    /// What the host future the coroutine waited for returned, or its panic,
    /// which goes on from here.
    fn resumed(resume: AsyncResume) -> Result<Vec<Value>, RuntimeError> {
        match resume {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::HostFuturePanicked(payload) => panic::resume_unwind(payload),
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        }
    }
}

fn run_immediate(mut future: HostFuture) -> Result<Vec<Value>, AsyncRuntimeError> {
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.map_err(AsyncRuntimeError::RuntimeError),
        Poll::Pending => Err(AsyncRuntimeError::YieldOutsideAsyncContext),
    }
}

/// A store of this backend, whichever backend `Store::default` picks: the
/// Windows test lane makes V8 the default, which has no async host functions.
#[cfg(test)]
fn sys_store() -> Store {
    Store::new(crate::backend::sys::entities::engine::default_engine())
}

#[cfg(test)]
mod stack_pool {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use corosensei::stack::Stack;

    use super::*;
    use crate::{Function, FunctionType};

    type Call = Pin<Box<dyn Future<Output = CallResult>>>;

    /// Pending until its flag is set.
    struct Gate(Rc<Cell<bool>>);
    impl Future for Gate {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.0.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    struct Host {
        store: StoreAsync,
        /// Waits for the gate, then returns.
        wait: Function,
        /// Panics.
        panic: Function,
        gate: Rc<Cell<bool>>,
        /// Where on the stack the last `wait` ran.
        ran_at: Rc<Cell<usize>>,
    }

    impl Host {
        fn new() -> Self {
            // Tests can share a thread (`--test-threads=1`).
            STACK_POOL.with(|pool| pool.borrow_mut().clear());
            let mut store = sys_store();
            let gate = Rc::new(Cell::new(false));
            let ran_at = Rc::new(Cell::new(0));
            let ty = FunctionType::new(vec![], vec![]);
            let wait = Function::new_async(&mut store, ty.clone(), {
                let (gate, ran_at) = (gate.clone(), ran_at.clone());
                move |_| {
                    let local = 0u8;
                    ran_at.set(ptr::from_ref(&local) as usize);
                    let gate = gate.clone();
                    async move {
                        Gate(gate).await;
                        Ok(vec![])
                    }
                }
            });
            let panic = Function::new(&mut store, ty, |_| panic!("host function panicked"));
            Self {
                store: store.into_async(),
                wait,
                panic,
                gate,
                ran_at,
            }
        }

        fn call(&self, function: &Function) -> Call {
            Box::pin(function.call_async(&self.store, vec![]))
        }
    }

    fn poll(call: &mut Call) -> Poll<CallResult> {
        call.as_mut()
            .poll(&mut Context::from_waker(&futures::task::noop_waker()))
    }

    /// The address range of each pooled stack.
    fn pooled() -> Vec<std::ops::Range<usize>> {
        STACK_POOL.with(|pool| {
            pool.borrow()
                .iter()
                .map(|stack| stack.limit().get()..stack.base().get())
                .collect()
        })
    }

    #[test]
    fn a_finished_call_leaves_its_stack_to_the_next_one() {
        let host = Host::new();
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        let first = pooled();
        assert_eq!(first.len(), 1, "the stack of the finished call is kept");
        assert!(first[0].contains(&host.ran_at.get()));

        host.gate.set(false);
        host.ran_at.set(0);
        let mut call = host.call(&host.wait);
        assert!(pooled().is_empty(), "the next call took it");
        assert!(poll(&mut call).is_pending());
        assert!(first[0].contains(&host.ran_at.get()), "and runs on it");
        // A call that overlaps it gets a stack of its own.
        let mut overlapping = host.call(&host.wait);
        assert!(poll(&mut overlapping).is_pending());
        assert!(!first[0].contains(&host.ran_at.get()));

        host.gate.set(true);
        assert!(matches!(poll(&mut call), Poll::Ready(Ok(_))));
        assert!(matches!(poll(&mut overlapping), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 2);
        assert!(pooled().contains(&first[0]));
    }

    #[test]
    fn the_pool_is_bounded() {
        let host = Host::new();
        let mut calls: Vec<Call> = (0..STACK_POOL_CAPACITY + 4)
            .map(|_| host.call(&host.wait))
            .collect();
        for call in &mut calls {
            assert!(poll(call).is_pending());
        }
        assert!(pooled().is_empty());
        host.gate.set(true);
        for call in &mut calls {
            assert!(matches!(poll(call), Poll::Ready(Ok(_))));
        }
        assert_eq!(pooled().len(), STACK_POOL_CAPACITY);
    }

    #[test]
    fn an_unwound_coroutine_does_not_give_its_stack_back() {
        let host = Host::new();

        // Dropped while suspended.
        let mut call = host.call(&host.wait);
        assert!(poll(&mut call).is_pending());
        drop(call);
        assert!(pooled().is_empty(), "dropped while suspended");

        // Unwound by a host panic.
        let mut call = host.call(&host.panic);
        assert!(catch_unwind(AssertUnwindSafe(|| poll(&mut call))).is_err());
        drop(call);
        assert!(pooled().is_empty(), "unwound by a host panic");

        // Never polled.
        drop(host.call(&host.wait));
        assert!(pooled().is_empty(), "never started");

        // The same with a stack that had been pooled: it is not put back.
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 1);
        host.gate.set(false);
        let mut call = host.call(&host.wait);
        assert!(poll(&mut call).is_pending());
        drop(call);
        let mut call = host.call(&host.panic);
        assert!(catch_unwind(AssertUnwindSafe(|| poll(&mut call))).is_err());
        drop(call);
        assert!(pooled().is_empty());

        // And calls keep working.
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 1);
    }
}

#[cfg(test)]
mod first_poll {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::{Function, FunctionEnv, FunctionEnvMut, FunctionType};

    /// Records, when dropped, whether the thread has a current coroutine and
    /// whether the store that is running is held by an async context.
    struct Observe(Arc<Mutex<Option<(bool, bool)>>>);
    impl Drop for Observe {
        fn drop(&mut self) {
            *self.0.lock().unwrap() = Some((
                CoroutineContext::get_current().is_some(),
                StoreContext::top_is_async(),
            ));
        }
    }

    /// A host future that panics in its first poll, which happens with the
    /// store lent out and the coroutine hidden, unwinds the host frames of
    /// the call with both back in place.
    #[test]
    fn a_panic_unwinds_the_call_with_its_store_and_coroutine_in_place() {
        let mut store = sys_store();
        let ty = FunctionType::new(vec![], vec![]);
        let panics = Function::new_async(&mut store, ty.clone(), |_| async {
            panic!("host future panicked")
        })
        .eager();
        let seen = Arc::new(Mutex::new(None));
        let env = FunctionEnv::new(&mut store, ());
        let outer = Function::new_with_env(&mut store, &env, ty, {
            let seen = seen.clone();
            move |mut env: FunctionEnvMut<()>, _| {
                let _observe = Observe(seen.clone());
                panics.call(&mut env, &[]).map(|_| vec![])
            }
        });
        let store = store.into_async();

        let mut call = Box::pin(outer.call_async(&store, vec![]));
        let poll = catch_unwind(AssertUnwindSafe(|| {
            call.as_mut()
                .poll(&mut Context::from_waker(&futures::task::noop_waker()))
        }));
        assert!(poll.is_err(), "the panic reaches the poller");
        assert_eq!(*seen.lock().unwrap(), Some((true, true)));
        drop(call);
        assert!(store.into_store().is_ok());
    }

    /// Locks the store and keeps the lock, then panics.
    struct PanicsWithTheStoreLocked<T> {
        lock: Pin<Box<dyn Future<Output = T>>>,
        held: Option<T>,
    }
    impl<T: Unpin> Future for PanicsWithTheStoreLocked<T> {
        type Output = Result<Vec<Value>, RuntimeError>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            this.held = Some(std::task::ready!(this.lock.as_mut().poll(cx)));
            panic!("host future panicked with the store locked")
        }
    }

    /// The same when the future holds the store's lock as it panics: the
    /// future is dropped, and the lock with it, before the store is taken
    /// back for the frames the panic unwinds through.
    #[test]
    fn a_panic_with_the_store_locked_unwinds_the_call_with_its_store_in_place() {
        let mut store = sys_store();
        let ty = FunctionType::new(vec![], vec![]);
        let env = FunctionEnv::new(&mut store, ());
        let panics = Function::new_with_env_async(
            &mut store,
            &env,
            ty.clone(),
            |env: crate::AsyncFunctionEnvMut<()>, _: &[Value]| PanicsWithTheStoreLocked {
                lock: Box::pin(async move { env.write().await }),
                held: None,
            },
        )
        .eager();
        let seen = Arc::new(Mutex::new(None));
        let outer = Function::new_with_env(&mut store, &env, ty, {
            let seen = seen.clone();
            move |mut env: FunctionEnvMut<()>, _| {
                let _observe = Observe(seen.clone());
                panics.call(&mut env, &[]).map(|_| vec![])
            }
        });
        let store = store.into_async();

        let mut call = Box::pin(outer.call_async(&store, vec![]));
        let poll = catch_unwind(AssertUnwindSafe(|| {
            call.as_mut()
                .poll(&mut Context::from_waker(&futures::task::noop_waker()))
        }));
        assert!(poll.is_err(), "the panic reaches the poller");
        assert_eq!(*seen.lock().unwrap(), Some((true, true)));
        drop(call);
        assert!(store.into_store().is_ok());
    }

    #[derive(Clone, Copy, PartialEq)]
    enum PanicsIn {
        Poll,
        Drop,
    }

    /// Locks the store and hands the lock over to `holder`. Then it panics,
    /// or is ready and panics when it is dropped.
    struct HandsTheStoreOver<T> {
        lock: Pin<Box<dyn Future<Output = T>>>,
        holder: Rc<RefCell<Option<T>>>,
        panics_in: PanicsIn,
    }
    impl<T> Future for HandsTheStoreOver<T> {
        type Output = Result<Vec<Value>, RuntimeError>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let lock = std::task::ready!(this.lock.as_mut().poll(cx));
            *this.holder.borrow_mut() = Some(lock);
            if this.panics_in == PanicsIn::Poll {
                panic!("host future panicked after handing the store over");
            }
            Poll::Ready(Ok(vec![]))
        }
    }
    impl<T> Drop for HandsTheStoreOver<T> {
        fn drop(&mut self) {
            if self.panics_in == PanicsIn::Drop && !std::thread::panicking() {
                panic!("host future panicked when dropped, after handing the store over");
            }
        }
    }

    /// The same when the future hands the store's lock to something else
    /// before it panics, in its poll or in its destructor: the call waits for
    /// the store, as it does when such a future returns, and the panic goes
    /// on once the store is back. The host function that catches it carries
    /// on with its store.
    fn after_handing_the_store_over(panics_in: PanicsIn) {
        let mut store = sys_store();
        let ty = FunctionType::new(vec![], vec![]);
        let env = FunctionEnv::new(&mut store, 0u32);
        let holder = Rc::new(RefCell::new(None));
        let panics = Function::new_with_env_async(&mut store, &env, ty.clone(), {
            let holder = holder.clone();
            move |env: crate::AsyncFunctionEnvMut<u32>, _: &[Value]| HandsTheStoreOver {
                lock: Box::pin(async move { env.write().await }),
                holder: holder.clone(),
                panics_in,
            }
        })
        .eager();
        let seen = Arc::new(Mutex::new(None));
        let outer = Function::new_with_env(&mut store, &env, ty, {
            let seen = seen.clone();
            move |mut env: FunctionEnvMut<u32>, _| {
                let caught = catch_unwind(AssertUnwindSafe(|| panics.call(&mut env, &[])));
                assert!(caught.is_err(), "the panic reaches the host function");
                *seen.lock().unwrap() = Some((
                    CoroutineContext::get_current().is_some(),
                    StoreContext::top_is_async(),
                ));
                *env.data_mut() += 1;
                Ok(vec![])
            }
        });
        let store = store.into_async();

        let mut call = Box::pin(outer.call_async(&store, vec![]));
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(
            call.as_mut().poll(&mut cx).is_pending(),
            "the call waits for the store"
        );
        assert_eq!(*seen.lock().unwrap(), None);
        holder.borrow_mut().take();
        assert!(matches!(call.as_mut().poll(&mut cx), Poll::Ready(Ok(_))));
        assert_eq!(*seen.lock().unwrap(), Some((true, true)));
        drop(call);
        let Ok(store) = store.into_store() else {
            panic!("the store is still shared");
        };
        assert_eq!(*env.as_ref(&store), 1);
    }

    #[test]
    fn a_panic_after_handing_the_store_over_goes_on_once_the_store_is_back() {
        after_handing_the_store_over(PanicsIn::Poll);
    }

    #[test]
    fn a_panic_dropping_a_future_that_handed_the_store_over_goes_on_once_the_store_is_back() {
        after_handing_the_store_over(PanicsIn::Drop);
    }

    /// Where an async host function's future is polled first: inside the
    /// import call, on the stack of the call, if the function is eager, and
    /// by the future of the call, on the executor's stack, if not.
    #[test]
    fn only_an_eager_host_future_is_polled_inside_the_import() {
        #[inline(never)]
        fn stack_position() -> usize {
            let local = 0u8;
            std::hint::black_box(ptr::from_ref(&local) as usize)
        }
        for eager in [false, true] {
            let mut store = sys_store();
            let (body, poll) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
            let f = Function::new_async(&mut store, FunctionType::new(vec![], vec![]), {
                let (body, poll) = (body.clone(), poll.clone());
                move |_| {
                    body.set(stack_position());
                    let poll = poll.clone();
                    std::future::poll_fn(move |_| {
                        poll.set(stack_position());
                        Poll::Ready(Ok(vec![]))
                    })
                }
            });
            let f = if eager { f.eager() } else { f };
            let store = store.into_async();
            let mut call = Box::pin(f.call_async(&store, vec![]));
            let waker = futures::task::noop_waker();
            let poll_result = call.as_mut().poll(&mut Context::from_waker(&waker));
            assert!(matches!(poll_result, Poll::Ready(Ok(_))));
            let here = stack_position();
            let on_the_stack_of_the_call = body.get().abs_diff(poll.get()) < here.abs_diff(poll.get());
            assert_eq!(on_the_stack_of_the_call, eager, "eager: {eager}");
        }
    }
}
