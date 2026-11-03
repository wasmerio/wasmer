use std::rc::Weak;
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    rc::Rc,
    sync::{Arc, LazyLock, atomic::AtomicU64},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use dashmap::DashMap;

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLock,
    LocalRwLockReadGuard, LocalRwLockWriteGuard, RuntimeError, Store, StoreAsync, StoreContext,
    StoreContextId, StoreInner, StoreMut, StoreRef, SuspendedStoreContexts, Value,
};
use wasmer_types::StoreId;

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;

pub(crate) fn call_function_async(
    function: SysFunction,
    store: StoreAsync,
    params: Vec<Value>,
) -> AsyncCallFuture {
    AsyncCallFuture::new(function, store, params)
}

struct AsyncYield(HostFuture);

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
}

type AsyncCallFutureId = u64;

static NEXT_ASYNC_CALL_FUTURE_ID: AtomicU64 = AtomicU64::new(0);
static NEXT_NATIVE_FRAME_ID: AtomicU64 = AtomicU64::new(0);

static ASYNC_CALL_FUTURE_WAKERS: LazyLock<
    DashMap<StoreId, HashMap<AsyncCallFutureId, RegisteredWaker>>,
> = LazyLock::new(DashMap::new);

struct RegisteredWaker {
    waker: Arc<Waker>,
    ancestors: Option<Arc<[AsyncCallFutureId]>>,
}

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    id: AsyncCallFutureId,
    store_id: StoreId,

    root: Option<Rc<NativeRootOwner>>,
    pending_resume_gate: Option<crate::WriteFuture<()>>,
    pending_frame_gate: Option<crate::WriteFuture<()>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,
}

/// An owner-thread generation for native invocations on one Store.
///
/// Close this generation before deactivating its attachments. Waiting observes
/// physical stack settlement, including stacks retained by escaped loan/frame
/// guards. The caller must then acquire the actual Store write lock before
/// deactivation. This observer owns no Store or coroutine and drives no cleanup.
#[doc(hidden)]
pub struct NativeInvocationScope {
    local: Rc<InvocationScopeLocal>,
}

struct InvocationScopeLocal {
    generation: Arc<NativeInvocationGeneration>,
    controls: RefCell<HashMap<AsyncCallFutureId, Weak<NativeRootControl>>>,
}

/// Only Send + Sync metadata is carried in StoreInner and async capabilities.
pub(crate) struct NativeInvocationGeneration {
    id: u64,
    store_id: StoreId,
    owner: std::thread::ThreadId,
    closed: std::sync::atomic::AtomicBool,
    pub(crate) superseded: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicUsize,
    reason: RuntimeError,
    waiter: std::sync::Mutex<Option<Arc<Waker>>>,
}

static NEXT_NATIVE_INVOCATION_SCOPE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static INVOCATION_SCOPES: RefCell<HashMap<u64, Weak<InvocationScopeLocal>>> = RefCell::new(HashMap::new());
}

impl NativeInvocationGeneration {
    pub(crate) fn check(&self) -> Result<(), RuntimeError> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(self.reason.clone());
        }
        if self.owner != std::thread::current().id() {
            return Err(RuntimeError::new(
                "native invocation generation belongs to another thread",
            ));
        }
        Ok(())
    }

    fn notify_settled(&self) {
        if !self.closed.load(std::sync::atomic::Ordering::Acquire)
            || self.active.load(std::sync::atomic::Ordering::Acquire) != 0
        {
            return;
        }
        let waiter = self.waiter.lock().unwrap().take();
        if let Some(waiter) = waiter {
            let mut failures = Vec::new();
            {
                let _logical = DetachedRootMetadata::new();
                defer_cleanup(&mut failures, || waiter.wake_by_ref());
            }
            defer_cleanup(&mut failures, || drop(waiter));
            resume_cleanup_panics(failures);
        }
    }
}

impl NativeInvocationScope {
    /// Bind before converting the exclusive Store into async capabilities.
    pub fn bind(store: &mut Store) -> Result<Self, RuntimeError> {
        if !store.inner.store.is_sys() {
            return Err(RuntimeError::new(
                "native invocation scopes require the sys backend",
            ));
        }
        if store.inner.native_invocation.is_some() {
            return Err(RuntimeError::new(
                "Store already has a native invocation generation",
            ));
        }
        let scope = Self::new(store.id());
        store.inner.native_invocation = Some(scope.local.generation.clone());
        Ok(scope)
    }

    fn new(store_id: StoreId) -> Self {
        let id = NEXT_NATIVE_INVOCATION_SCOPE
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |id| id.checked_add(1),
            )
            .expect("native invocation generation identity exhausted");
        let local = Rc::new(InvocationScopeLocal {
            generation: Arc::new(NativeInvocationGeneration {
                id,
                store_id,
                owner: std::thread::current().id(),
                closed: std::sync::atomic::AtomicBool::new(false),
                superseded: std::sync::atomic::AtomicBool::new(false),
                active: std::sync::atomic::AtomicUsize::new(0),
                reason: RuntimeError::host_abort(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "native invocation generation closed",
                ))),
                waiter: std::sync::Mutex::new(None),
            }),
            controls: RefCell::new(HashMap::new()),
        });
        INVOCATION_SCOPES.with(|scopes| {
            assert!(
                scopes
                    .borrow_mut()
                    .insert(id, Rc::downgrade(&local))
                    .is_none()
            );
        });
        Self { local }
    }

    /// Reject all new entries and cancel every existing root in this generation.
    /// Waker callbacks run after all metadata borrows have ended.
    pub fn close(&self) {
        let generation = &self.local.generation;
        if generation
            .closed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let controls: Vec<_> = self
            .local
            .controls
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        for control in &controls {
            if !control.settled.get() {
                control.retire(Some(generation.reason.clone()));
            }
        }
        let ids: Vec<_> = controls.iter().map(|control| control.future_id).collect();
        let wakers: Vec<_> = ASYNC_CALL_FUTURE_WAKERS
            .iter()
            .flat_map(|entry| {
                entry
                    .iter()
                    .filter(|(id, record)| {
                        ids.contains(id)
                            || record
                                .ancestors
                                .as_ref()
                                .is_some_and(|parents| parents.iter().any(|id| ids.contains(id)))
                    })
                    .map(|(_, record)| record.waker.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        let mut failures = Vec::new();
        {
            let _logical = DetachedRootMetadata::new();
            for waker in &wakers {
                defer_cleanup(&mut failures, || waker.wake_by_ref());
            }
        }
        for waker in wakers {
            defer_cleanup(&mut failures, || drop(waker));
        }
        defer_cleanup(&mut failures, || generation.notify_settled());
        resume_cleanup_panics(failures);
    }

    /// Close and wait outside every root whose lifetime retains this scope.
    /// Escaped guards held by the caller must be released before this wait.
    pub async fn close_and_wait(&mut self) -> Result<(), RuntimeError> {
        self.close();
        let current = CoroutineContext::current_root()
            .map(|root| root.control)
            .or_else(|| RETIRING_ROOT.with(|root| root.borrow().clone()));
        if current.is_some_and(|control| control.contains_scope(self.local.generation.id)) {
            return Err(RuntimeError::new(
                "cannot wait for the current native invocation lineage",
            ));
        }
        self.wait_for_settlement().await;
        Ok(())
    }

    /// Close and observe settlement from separately owned host cleanup.
    ///
    /// Unlike close_and_wait, this observer may be polled inline while a
    /// dependent native root is active. It grants no Store or guest authority.
    /// The cleanup future must be owned by the external executor, never awaited
    /// by a root whose physical lifetime it observes; that would deadlock.
    #[doc(hidden)]
    pub async fn close_and_wait_for_cleanup(&mut self) {
        self.close();
        self.wait_for_settlement().await;
    }

    async fn wait_for_settlement(&mut self) {
        let generation = &self.local.generation;
        let _registration = ScopeWaiterRegistration(generation.clone());
        std::future::poll_fn(|cx| {
            if generation.active.load(std::sync::atomic::Ordering::Acquire) == 0 {
                return Poll::Ready(());
            }
            // Clone before locking: RawWaker clone/Drop may call arbitrary code.
            let next = Arc::new(cx.waker().clone());
            let old = generation.waiter.lock().unwrap().replace(next);
            drop(old);
            if generation.active.load(std::sync::atomic::Ordering::Acquire) == 0 {
                let waiter = generation.waiter.lock().unwrap().take();
                drop(waiter);
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    /// Begin a new image generation under the actual Store write guard.
    /// Old strong and weak async capabilities remain closed permanently.
    pub fn successor(
        &self,
        base: &StoreAsync,
        store: &mut StoreMut<'_>,
    ) -> Result<(Self, StoreAsync), RuntimeError> {
        let old = &self.local.generation;
        if old.owner != std::thread::current().id()
            || !old.closed.load(std::sync::atomic::Ordering::Acquire)
            || old.active.load(std::sync::atomic::Ordering::Acquire) != 0
            || old.store_id != store.as_store_ref().objects().id()
            || base.loan.is_some()
            || !base
                .native_invocation
                .as_ref()
                .is_some_and(|generation| Arc::ptr_eq(generation, old))
            || !store
                .inner
                .native_invocation
                .as_ref()
                .is_some_and(|generation| Arc::ptr_eq(generation, old))
        {
            return Err(RuntimeError::new(
                "native invocation generation cannot be replaced yet",
            ));
        }
        let scope = Self::new(old.store_id);
        // No callbacks occur between sealing the old consumption authority and
        // publishing the new generation under the exact Store write guard.
        old.superseded
            .store(true, std::sync::atomic::Ordering::Release);
        store.inner.native_invocation = Some(scope.local.generation.clone());
        let mut next = base.store();
        next.native_invocation = Some(scope.local.generation.clone());
        Ok((scope, next))
    }
}

impl Drop for NativeInvocationScope {
    fn drop(&mut self) {
        self.close();
    }
}

impl Drop for InvocationScopeLocal {
    fn drop(&mut self) {
        // This final Rc field also drops if the owner's close notification panics.
        // During TLS teardown an already destroyed registry has no row to remove.
        let removed = INVOCATION_SCOPES
            .try_with(|scopes| scopes.borrow_mut().remove(&self.generation.id))
            .ok()
            .flatten();
        drop(removed);
    }
}

struct RootInvocationScope {
    local: Weak<InvocationScopeLocal>,
    generation: Arc<NativeInvocationGeneration>,
    started: Cell<bool>,
}

// A canceled wait must not retain its task until an escaped frame settles.
struct ScopeWaiterRegistration(Arc<NativeInvocationGeneration>);
impl Drop for ScopeWaiterRegistration {
    fn drop(&mut self) {
        let waiter = self.0.waiter.lock().unwrap().take();
        drop(waiter);
    }
}

// Count changes remain callback-free. The outer poll/final-owner wrapper calls
// this only after inner stack, TLS and Store guards have left, and combines a
// notification panic with any pending cleanup panic before propagating either.
fn notify_scope_after<T>(
    generation: Option<Arc<NativeInvocationGeneration>>,
    outcome: Result<T, DeferredPanic>,
) -> T {
    let mut failures = Vec::new();
    let mut value = match outcome {
        Ok(value) => Some(value),
        Err(panic) => {
            failures.push(panic);
            None
        }
    };
    if let Some(generation) = generation {
        defer_cleanup(&mut failures, || generation.notify_settled());
    }
    if !failures.is_empty() {
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        defer_cleanup(&mut failures, || drop(value.take()));
        resume_cleanup_panics(failures);
    }
    value.unwrap()
}

/// The stack and its raw TLS snapshots share one physical lifetime. Host futures
/// remain outside this owner, so capturing a weak root cannot create a cycle.
struct NativeRootOwner {
    store: Option<StoreAsync>,
    store_context_id: StoreContextId,
    control: Rc<NativeRootControl>,
    parked: Cell<bool>,
    loan_gate: OnceCell<LocalRwLock<()>>,
    frame_gate: OnceCell<LocalRwLock<()>>,
    frame_resume_waiting: Cell<bool>,
    stack: RefCell<NativeRootStack>,
}

#[allow(clippy::type_complexity)]
struct NativeRootStack {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    suspended_stores: SuspendedStoreContexts,
    trap_state: RootTrapState,
}

#[derive(Clone)]
struct WeakNativeRoot {
    owner: Weak<NativeRootOwner>,
    id: StoreContextId,
    control: Rc<NativeRootControl>,
}

/// This metadata may outlive the stack. It owns no Store, coroutine or task.
struct NativeRootControl {
    future_id: AsyncCallFutureId,
    scope: Option<RootInvocationScope>,
    ancestors: Option<Arc<[AsyncCallFutureId]>>,
    store_id: StoreId,
    installation: StoreContextId,
    retiring: Cell<bool>,
    settled: Cell<bool>,
    retained: Cell<bool>,
    parent: Option<Weak<Self>>,
    native_edges: RefCell<Vec<NativeParent>>,
    native_reason: OnceCell<RuntimeError>,
    previous_row: RefCell<Option<Weak<Self>>>,
    reason: OnceCell<RuntimeError>,
    terminal: OnceCell<RuntimeError>,
}

struct NativeParent {
    id: u64,
    parent: Weak<NativeRootControl>,
}

thread_local! {
    // A retained original write guard makes this a single-owner association.
    // Weak metadata cannot keep a coroutine, Store or cleanup task alive.
    static RETAINED_ROOTS: RefCell<HashMap<StoreId, Weak<NativeRootControl>>> = RefCell::new(HashMap::new());
    static RETIRING_ROOT: RefCell<Option<Rc<NativeRootControl>>> = const { RefCell::new(None) };
}

// Destructors run without a coroutine yielder or Store authority. Keep only
// the exact cancellation metadata available to managed entry checks.
struct RetirementBoundary {
    previous: Option<Rc<NativeRootControl>>,
    installed: Option<Rc<NativeRootControl>>,
}

impl RetirementBoundary {
    fn enter(control: &Rc<NativeRootControl>) -> Self {
        let installed = (!control.settled.get()).then(|| control.clone());
        let previous = installed.as_ref().and_then(|control| {
            RETIRING_ROOT.with(|current| current.borrow_mut().replace(control.clone()))
        });
        Self {
            previous,
            installed,
        }
    }
}

impl Drop for RetirementBoundary {
    fn drop(&mut self) {
        if let Some(installed) = self.installed.take() {
            let removed = RETIRING_ROOT.with(|current| {
                let mut current = current.borrow_mut();
                assert!(
                    current
                        .as_ref()
                        .is_some_and(|found| Rc::ptr_eq(found, &installed)),
                    "retirement metadata restored out of order"
                );
                std::mem::replace(&mut *current, self.previous.take())
            });
            drop(removed);
        }
    }
}

// Waking an executor may poll several unrelated tasks inline. Only the explicit
// child lineage is notified, and ambient logical cancellation is hidden while
// wake callbacks run. Waker destructors run after the metadata is restored.
struct DetachedRootMetadata(Option<Rc<NativeRootControl>>);
impl DetachedRootMetadata {
    fn new() -> Self {
        Self(RETIRING_ROOT.with(|current| current.borrow_mut().take()))
    }
}
impl Drop for DetachedRootMetadata {
    fn drop(&mut self) {
        RETIRING_ROOT.with(|current| {
            let mut current = current.borrow_mut();
            assert!(
                current.is_none(),
                "wake callback left logical metadata installed"
            );
            *current = self.0.take();
        });
    }
}

impl NativeRootControl {
    fn contains_scope(self: &Rc<Self>, id: u64) -> bool {
        let mut pending = vec![self.clone()];
        let mut visited = Vec::new();
        while let Some(control) = pending.pop() {
            if visited.contains(&control.future_id) {
                continue;
            }
            visited.push(control.future_id);
            if control
                .scope
                .as_ref()
                .is_some_and(|scope| scope.generation.id == id)
            {
                return true;
            }
            if let Some(parent) = control.parent.as_ref().and_then(Weak::upgrade) {
                pending.push(parent);
            }
            pending.extend(
                control
                    .native_edges
                    .borrow()
                    .iter()
                    .filter_map(|edge| edge.parent.upgrade()),
            );
        }
        false
    }

    fn start_scope(&self) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.scope {
            scope.generation.check()?;
            if !scope.started.get() {
                scope
                    .generation
                    .active
                    .fetch_update(
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                        |count| count.checked_add(1),
                    )
                    .expect("native physical invocation count exhausted");
                scope.started.set(true);
            }
        }
        Ok(())
    }

    fn retire(&self, managed_reason: Option<RuntimeError>) {
        let ancestor = self
            .error()
            .or_else(|| managed_reason.filter(RuntimeError::is_host_abort));
        self.reason.get_or_init(|| {
            ancestor.unwrap_or_else(|| {
                RuntimeError::host_abort(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "native call cancelled",
                )))
            })
        });
        self.retiring.set(true);
    }

    fn error(&self) -> Option<RuntimeError> {
        let error = if self.has_native_lineage() {
            self.error_in_lineage(&mut Vec::new())
        } else {
            self.error_without_native_lineage()
        };
        error.or_else(|| {
            (!self.settled.get())
                .then(|| {
                    self.scope
                        .as_ref()
                        .and_then(|scope| scope.generation.check().err())
                })
                .flatten()
        })
    }

    fn has_native_lineage(&self) -> bool {
        !self.native_edges.borrow().is_empty()
            || self
                .parent
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|parent| parent.has_native_lineage())
    }

    fn error_without_native_lineage(&self) -> Option<RuntimeError> {
        if self.settled.get() {
            return self.terminal.get().cloned();
        }
        if let Some(parent) = self.parent.as_ref().and_then(Weak::upgrade)
            && let Some(error) = parent.error()
        {
            return Some(error);
        }
        if let Some(error) = self.terminal.get().or_else(|| self.native_reason.get()) {
            return Some(error.clone());
        }
        self.retiring.get().then(|| {
            self.reason
                .get()
                .expect("retirement reason precedes its flag")
                .clone()
        })
    }

    fn error_in_lineage(&self, visited: &mut Vec<AsyncCallFutureId>) -> Option<RuntimeError> {
        if self.settled.get() {
            return self.terminal.get().cloned();
        }
        if visited.contains(&self.future_id) {
            return None;
        }
        visited.push(self.future_id);
        if let Some(parent) = self.parent.as_ref().and_then(Weak::upgrade)
            && let Some(error) = parent.error_in_lineage(visited)
        {
            return Some(error);
        }
        for edge in self.native_edges.borrow().iter().rev() {
            if let Some(parent) = edge.parent.upgrade()
                && let Some(error) = parent.error_in_lineage(visited)
            {
                return Some(error);
            }
        }
        self.terminal
            .get()
            .or_else(|| self.native_reason.get())
            .cloned()
            .or_else(|| {
                self.retiring.get().then(|| {
                    self.reason
                        .get()
                        .expect("retirement reason precedes its flag")
                        .clone()
                })
            })
    }

    fn combined_ancestors(self: &Rc<Self>) -> Option<Arc<[AsyncCallFutureId]>> {
        if !self.has_native_lineage() {
            return self.ancestors.clone();
        }
        let mut ids = self.ancestors.as_deref().unwrap_or(&[]).to_vec();
        let mut visited = Vec::new();
        let mut pending = vec![self.clone()];
        while let Some(control) = pending.pop() {
            if visited.contains(&control.future_id) {
                continue;
            }
            visited.push(control.future_id);
            if control.future_id != self.future_id && !ids.contains(&control.future_id) {
                ids.push(control.future_id);
            }
            if let Some(parent) = control.parent.as_ref().and_then(Weak::upgrade) {
                pending.push(parent);
            }
            pending.extend(
                control
                    .native_edges
                    .borrow()
                    .iter()
                    .filter_map(|edge| edge.parent.upgrade()),
            );
        }
        (!ids.is_empty()).then(|| Arc::from(ids))
    }

    fn contains_ancestor(self: &Rc<Self>, target: &Rc<Self>) -> bool {
        let mut visited = Vec::new();
        let mut pending = vec![self.clone()];
        while let Some(control) = pending.pop() {
            if Rc::ptr_eq(&control, target) {
                return true;
            }
            if visited.contains(&control.future_id) {
                continue;
            }
            visited.push(control.future_id);
            if let Some(parent) = control.parent.as_ref().and_then(Weak::upgrade) {
                pending.push(parent);
            }
            pending.extend(
                control
                    .native_edges
                    .borrow()
                    .iter()
                    .filter_map(|edge| edge.parent.upgrade()),
            );
        }
        false
    }

    fn refresh_registered_ancestors(self: &Rc<Self>) {
        replace_registered_ancestors(self.store_id, self.future_id, self.combined_ancestors());
    }

    fn settle(&self) {
        // A parent edge is weak. Preserve its exact terminal error before the
        // owned parent guard can release the last strong parent stack owner.
        if let Some(error) = self.error() {
            self.terminal.get_or_init(|| error);
        }
        if !self.settled.replace(true)
            && let Some(scope) = &self.scope
        {
            if let Some(local) = scope.local.upgrade() {
                local.controls.borrow_mut().remove(&self.future_id);
            }
            if scope.started.replace(false) {
                let previous = scope
                    .generation
                    .active
                    .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                assert!(previous > 0, "native physical invocation count underflow");
            }
        }
    }

    fn register(self: &Rc<Self>, stores: &SuspendedStoreContexts) {
        assert_eq!(
            stores.owned_root_identity(),
            Some((self.store_id, self.installation))
        );
        let previous = RETAINED_ROOTS.with(|roots| roots.borrow().get(&self.store_id).cloned());
        let expected = Rc::downgrade(self);
        if self.retained.get() {
            assert!(
                previous
                    .as_ref()
                    .is_some_and(|row| Weak::ptr_eq(row, &expected)),
                "retained native root row changed"
            );
            return;
        }
        let prior = if let Some(parent) = &self.parent {
            assert!(
                previous
                    .as_ref()
                    .is_some_and(|row| Weak::ptr_eq(row, parent)),
                "native child did not acquire its exact parent row"
            );
            let owner = parent
                .upgrade()
                .expect("parent guard keeps its control alive");
            assert!(owner.retained.get() && owner.store_id == self.store_id);
            previous
        } else {
            assert!(
                previous.as_ref().and_then(Weak::upgrade).is_none(),
                "an unrelated native root owns this Store"
            );
            None
        };
        assert!(self.previous_row.borrow().is_none());
        *self.previous_row.borrow_mut() = prior;
        let old = RETAINED_ROOTS.with(|roots| roots.borrow_mut().insert(self.store_id, expected));
        self.retained.set(true);
        drop(old);
    }

    fn unregister(self: &Rc<Self>) {
        if !self.retained.get() {
            return;
        }
        let expected = Rc::downgrade(self);
        let prior = self.previous_row.borrow().clone();
        if let Some(prior) = &prior {
            let owner = prior
                .upgrade()
                .expect("owned parent guard outlives child retirement");
            assert!(owner.retained.get() && owner.store_id == self.store_id);
        }
        let old = RETAINED_ROOTS.with(|roots| {
            let mut roots = roots.borrow_mut();
            assert!(
                roots
                    .get(&self.store_id)
                    .is_some_and(|row| Weak::ptr_eq(row, &expected)),
                "retained Store retirement owner changed"
            );
            match prior {
                Some(prior) => roots.insert(self.store_id, prior),
                None => roots.remove(&self.store_id),
            }
        });
        self.retained.set(false);
        let saved = self.previous_row.borrow_mut().take();
        drop(saved);
        drop(old);
    }
}

fn current_retirement_control() -> Option<Rc<NativeRootControl>> {
    if let Some(current) = CoroutineContext::get_current() {
        // SAFETY: installed pointers belong to the currently executing native stack.
        let context = unsafe { &*current };
        if let Some(root) = &context.owner {
            assert_eq!(context.store_context_id, root.control.installation);
            // The physical root is authoritative even when it has no error.
            return Some(root.control.clone());
        }
    }
    RETIRING_ROOT.with(|current| {
        current
            .borrow()
            .clone()
            .filter(|control| !control.settled.get())
    })
}

pub(crate) fn current_retirement_error() -> Option<RuntimeError> {
    current_retirement_control().and_then(|control| control.error())
}

pub(crate) fn store_retirement_error(store_id: StoreId) -> Option<RuntimeError> {
    let weak = RETAINED_ROOTS.with(|roots| roots.borrow().get(&store_id).cloned())?;
    let control = weak.upgrade()?;
    assert_eq!(control.store_id, store_id);
    if !control.retained.get() {
        return None;
    }
    control.error()
}

// This token is intentionally not Clone. A future capability may be copied as
// a WeakNativeRoot; strong custody belongs only to an actual outside operation.
struct NativeRootPin {
    _owner: Rc<NativeRootOwner>,
}

impl WeakNativeRoot {
    fn try_pin(&self) -> Option<NativeRootPin> {
        // Do this before touching stack state. A pin on its own coroutine stack
        // would make the owner retain itself, and that stack is currently borrowed.
        if CoroutineContext::get_current()
            .is_some_and(|context| unsafe { &*context }.store_context_id == self.id)
        {
            return None;
        }
        let owner = self.owner.upgrade()?;
        if owner.store_context_id != self.id || !Rc::ptr_eq(&owner.control, &self.control) {
            return None;
        }
        if !owner.parked.get() || owner.control.error().is_some() || owner.control.settled.get() {
            return None;
        }
        Some(NativeRootPin { _owner: owner })
    }
}

/// A weak identity for a native invocation, without Store or yielder access.
///
/// External engines use this token only to describe physical frame ancestry.
#[doc(hidden)]
#[derive(Clone)]
pub struct WeakNativeInvocation {
    root: WeakNativeRoot,
}

impl WeakNativeInvocation {
    /// Capture the currently executing native coroutine, if one exists.
    pub fn current() -> Option<Self> {
        CoroutineContext::current_root().map(|root| Self { root })
    }

    /// Whether this is the exact currently executing physical coroutine.
    /// Retirement does not change identity; a parked or ended root is not current.
    pub fn is_current(&self) -> bool {
        CoroutineContext::current_root().is_some_and(|current| {
            current.id == self.root.id && Rc::ptr_eq(&current.control, &self.root.control)
        })
    }

    /// The exact managed cancellation inherited by this invocation.
    pub fn retirement_error(&self) -> Option<RuntimeError> {
        self.root.control.error()
    }

    /// Block this invocation while an external descendant frame is active.
    ///
    /// Same-invocation entry owns no pin. With no current coroutine, the guard
    /// still blocks this ancestor and exposes its cancellation for transport.
    ///
    /// # Safety
    /// This token must identify the actual preceding external-engine frame.
    /// Drop the returned guard only after exiting the descendant engine's
    /// thread-local entry scopes, including partially constructed instances.
    /// Guards describing nested engine frames must be destroyed in LIFO order.
    /// With no current coroutine, this blocks only the ancestor: the unmanaged
    /// caller must reject independently parking descendants, or keep its own
    /// frame active until all such descendants have physically settled.
    pub unsafe fn enter_descendant_frame(&self) -> Result<NativeFrameGuard, RuntimeError> {
        if let Some(error) = current_retirement_error().or_else(|| self.retirement_error()) {
            return Err(error);
        }
        let current = CoroutineContext::current_root();
        if current.as_ref().is_some_and(|root| root.id == self.root.id) {
            return Ok(NativeFrameGuard {
                binding: None,
                _gate: None,
                _root: None,
                ancestor: self.clone(),
            });
        }
        if let Some(current) = &current
            && self.root.control.contains_ancestor(&current.control)
        {
            return Err(RuntimeError::new(
                "native frame ancestry would form a cycle",
            ));
        }
        let root = self.root.try_pin().ok_or_else(|| {
            self.retirement_error()
                .unwrap_or_else(|| RuntimeError::new("native ancestor is no longer parked"))
        })?;
        if root._owner.frame_resume_waiting.get() {
            return Err(RuntimeError::new("native ancestor is waiting to resume"));
        }
        let gate = root._owner.frame_gate.get_or_init(|| LocalRwLock::new(()));
        // Never synchronously wait from an external engine's scope constructor.
        let gate = gate
            .try_read()
            .ok_or_else(|| RuntimeError::new("native ancestor is waiting to resume"))?;
        let mut guard = NativeFrameGuard {
            binding: None,
            _gate: Some(gate),
            _root: Some(root),
            ancestor: self.clone(),
        };
        if let Some(current) = current {
            guard.binding = Some(NativeFrameBinding::enter(
                current.control,
                &self.root.control,
            )?);
        }
        Ok(guard)
    }
}

/// Synchronous custody for an inert external-engine cleanup frame.
///
/// This guard supplies no Store access and no native cancellation ancestry.
/// It blocks a preceding parked invocation's resume/final driver while cleanup
/// runs, including when that invocation is already retiring. New async roots
/// cannot enter, and existing synchronous calls must complete without yielding.
#[doc(hidden)]
pub struct NativeCleanupGuard {
    no_yield: Option<NativeCleanupNoYield>,
    gate: Option<LocalRwLockReadGuard<()>>,
    root: Option<NativeRootPin>,
}

impl NativeCleanupGuard {
    /// Enter a host-only provider cleanup frame without admitting guest work.
    ///
    /// A queued resume writer may be joined only when its read gate is
    /// immediately available. A held writer with a still-parked root belongs
    /// to the synchronous enclosing runtime poll: cleanup returns before that
    /// poll continues. That case retains the pin and NoYield custody without
    /// claiming a read guard. The current physical retirement driver needs no
    /// self-pin, including after its final strong owner has begun dropping.
    ///
    /// # Safety
    /// `previous` must identify the actual preceding external-engine frame, or
    /// be `None` when it has no managed invocation. Use this only for inert host
    /// disposal; managed guest finalizers require ordinary frame admission and
    /// failure transport. Keep the guard until the cleanup isolate has exited
    /// and its preceding engine-frame header is restored. Nested guards must
    /// leave in LIFO order. This synchronous guard must be destroyed before
    /// returning from a resume-lock callback; it cannot be parked or transferred
    /// to a later cleanup task. An unmanaged caller must finish all synchronous
    /// descendants before returning; this guard grants no parking authority.
    pub unsafe fn enter(previous: Option<&WeakNativeInvocation>) -> Result<Self, RuntimeError> {
        let mut guard = Self {
            no_yield: None,
            gate: None,
            root: None,
        };
        if let Some(previous) = previous
            && !previous.is_current()
        {
            let owner =
                previous.root.owner.upgrade().ok_or_else(|| {
                    RuntimeError::new("native cleanup ancestor has already ended")
                })?;
            if owner.store_context_id != previous.root.id
                || !Rc::ptr_eq(&owner.control, &previous.root.control)
                || !owner.parked.get()
                || owner.control.settled.get()
            {
                return Err(RuntimeError::new("native cleanup ancestor is not parked"));
            }
            let gate = owner.frame_gate.get_or_init(|| LocalRwLock::new(()));
            guard.gate = gate.try_read();
            if guard.gate.is_none() && !owner.frame_resume_waiting.get() {
                return Err(RuntimeError::new(
                    "native cleanup ancestor has no resume custody",
                ));
            }
            // The only frame writer is the enclosing AsyncCallFuture::poll.
            // Its parked/waiting state above proves synchronous caller custody
            // when try_read found a writer. No callback or suspension occurs
            // between that check and publishing this final-driver pin.
            guard.root = Some(NativeRootPin { _owner: owner });
        }
        guard.no_yield = Some(NativeCleanupNoYield::enter());
        Ok(guard)
    }
}

impl Drop for NativeCleanupGuard {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        let mut failures = Vec::new();
        // The caller has exited/restored the actual engine frame before Drop.
        // Remove NoYield before gate/pin release can wake another invocation.
        defer_cleanup(&mut failures, || drop(self.no_yield.take()));
        defer_cleanup(&mut failures, || drop(self.gate.take()));
        defer_cleanup(&mut failures, || drop(self.root.take()));
        resume_guard_cleanup_panics(failures, already_unwinding);
    }
}

struct NativeCleanupNoYield {
    previous: usize,
}

impl NativeCleanupNoYield {
    fn enter() -> Self {
        NATIVE_CLEANUP_DEPTH.with(|depth| {
            let previous = depth.get();
            depth.set(
                previous
                    .checked_add(1)
                    .expect("native cleanup depth exhausted"),
            );
            Self { previous }
        })
    }
}

impl Drop for NativeCleanupNoYield {
    fn drop(&mut self) {
        NATIVE_CLEANUP_DEPTH.with(|depth| {
            assert_eq!(
                depth.get(),
                self.previous + 1,
                "native cleanup dropped out of order"
            );
            depth.set(self.previous);
        });
    }
}

pub(crate) struct MemoryGrowCallbackGuard {
    control: Option<Rc<NativeRootControl>>,
    _no_yield: NativeCleanupNoYield,
}
impl MemoryGrowCallbackGuard {
    pub(crate) fn enter() -> Self {
        Self {
            control: current_retirement_control(),
            _no_yield: NativeCleanupNoYield::enter(),
        }
    }
    pub(crate) fn retirement_error(&self) -> Option<RuntimeError> {
        self.control.as_ref().and_then(|control| control.error())
    }
}

fn native_cleanup_no_yield() -> bool {
    NATIVE_CLEANUP_DEPTH.with(|depth| depth.get() != 0)
}

/// Custody for a descendant external-engine frame.
///
/// It carries no Store access. Exiting the engine's thread-local scopes must
/// precede dropping this guard; its final pin may retire a parked ancestor.
#[doc(hidden)]
pub struct NativeFrameGuard {
    // Latch and remove the child edge, unlock resume, then release the final pin.
    binding: Option<NativeFrameBinding>,
    _gate: Option<LocalRwLockReadGuard<()>>,
    _root: Option<NativeRootPin>,
    ancestor: WeakNativeInvocation,
}

impl Drop for NativeFrameGuard {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        let mut failures = Vec::new();
        // Finish every ownership phase before replaying the first panic. A gate
        // Waker and the final root driver's stack payload can both panic.
        defer_cleanup(&mut failures, || drop(self.binding.take()));
        defer_cleanup(&mut failures, || drop(self._gate.take()));
        defer_cleanup(&mut failures, || drop(self._root.take()));
        resume_guard_cleanup_panics(failures, already_unwinding);
    }
}

impl NativeFrameGuard {
    /// Managed ancestor cancellation, including when no coroutine is current.
    pub fn retirement_error(&self) -> Option<RuntimeError> {
        self.ancestor.retirement_error()
    }

    /// Inherit this weak ancestry through a frame without its own coroutine.
    pub fn ancestor(&self) -> WeakNativeInvocation {
        self.ancestor.clone()
    }
}

struct NativeFrameBinding {
    id: u64,
    child: Rc<NativeRootControl>,
}

impl NativeFrameBinding {
    fn enter(
        child: Rc<NativeRootControl>,
        parent: &Rc<NativeRootControl>,
    ) -> Result<Self, RuntimeError> {
        if parent.contains_ancestor(&child) {
            return Err(RuntimeError::new(
                "native frame ancestry would form a cycle",
            ));
        }
        let id = NEXT_NATIVE_FRAME_ID
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |id| id.checked_add(1),
            )
            .expect("native frame identity exhausted");
        child.native_edges.borrow_mut().push(NativeParent {
            id,
            parent: Rc::downgrade(parent),
        });
        // Construct custody before metadata publication so a later panic also
        // restores the exact edge. Updating IDs never clones a user Waker.
        let binding = Self { id, child };
        binding.child.refresh_registered_ancestors();
        Ok(binding)
    }
}

impl Drop for NativeFrameBinding {
    fn drop(&mut self) {
        if let Some(error) = self.child.error() {
            assert!(
                error.is_host_abort(),
                "native ancestry carries only managed aborts"
            );
            self.child.native_reason.get_or_init(|| error);
        }
        let removed = {
            let mut edges = self.child.native_edges.borrow_mut();
            assert_eq!(
                edges.last().map(|edge| edge.id),
                Some(self.id),
                "native frame edge dropped out of order"
            );
            edges.pop()
        };
        drop(removed);
        self.child.refresh_registered_ancestors();
    }
}

/// A weak authority for one still-live derived synchronous borrow. The unit
/// lock owns no Store or coroutine; only an actual outside guard pins the stack.
#[derive(Clone)]
pub(crate) struct NativeStoreLoan {
    root: WeakNativeRoot,
    scope: StoreContextId,
    epoch: Rc<Cell<bool>>,
    gate: LocalRwLock<()>,
    store_id: StoreId,
}

pub(crate) struct NativeLoanReadGuard {
    pointer: *mut StoreInner,
    _gate: Option<LocalRwLockReadGuard<()>>,
    _root: Option<NativeRootPin>,
}

pub(crate) struct NativeLoanWriteGuard {
    pointer: *mut StoreInner,
    _gate: Option<LocalRwLockWriteGuard<()>>,
    _root: Option<NativeRootPin>,
}

impl Drop for NativeLoanReadGuard {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        let mut failures = Vec::new();
        defer_cleanup(&mut failures, || drop(self._gate.take()));
        defer_cleanup(&mut failures, || drop(self._root.take()));
        resume_guard_cleanup_panics(failures, already_unwinding);
    }
}

impl NativeLoanReadGuard {
    pub(crate) fn store(&self) -> &StoreInner {
        // SAFETY: the read gate prevents parent resume and mutable children;
        // the pin retains the original guard and the pointer's exact ancestry.
        unsafe { &*self.pointer }
    }
}

impl Drop for NativeLoanWriteGuard {
    fn drop(&mut self) {
        let already_unwinding = std::thread::panicking();
        let mut failures = Vec::new();
        defer_cleanup(&mut failures, || drop(self._gate.take()));
        defer_cleanup(&mut failures, || drop(self._root.take()));
        resume_guard_cleanup_panics(failures, already_unwinding);
    }
}

impl NativeLoanWriteGuard {
    pub(crate) fn store(&self) -> &StoreInner {
        // SAFETY: the exclusive gate and root pin cover this shared reborrow.
        unsafe { &*self.pointer }
    }

    pub(crate) fn context_pointer(&self) -> *mut StoreInner {
        // Copy the exact derived pointer without reborrowing its ancestor guard.
        self.pointer
    }

    pub(crate) fn store_mut(&mut self) -> &mut StoreInner {
        // SAFETY: only this guard has the exclusive gate; the parent is parked.
        unsafe { &mut *self.pointer }
    }
}

impl NativeStoreLoan {
    pub(crate) fn is_live(&self) -> bool {
        self.epoch.get()
            && self.root.owner.upgrade().is_some_and(|owner| {
                owner.control.error().is_none() && !owner.control.settled.get()
            })
    }

    fn pin(&self) -> Option<NativeRootPin> {
        if !self.epoch.get() {
            return None;
        }
        let pin = self.root.try_pin()?;
        if !pin._owner.parked.get() {
            return None;
        }
        Some(pin)
    }

    fn pointer(&self, pin: &NativeRootPin) -> Option<*mut StoreInner> {
        let stack = pin._owner.stack.borrow();
        stack
            .suspended_stores
            .borrowed_pointer(self.scope, self.store_id)
    }

    pub(crate) fn try_read(&self) -> Option<NativeLoanReadGuard> {
        let root = self.pin()?;
        let gate = self.gate.try_read()?;
        let pointer = self.pointer(&root)?;
        Some(NativeLoanReadGuard {
            pointer,
            _gate: Some(gate),
            _root: Some(root),
        })
    }

    pub(crate) fn try_write(&self) -> Option<NativeLoanWriteGuard> {
        let root = self.pin()?;
        let gate = self.gate.try_write()?;
        let pointer = self.pointer(&root)?;
        Some(NativeLoanWriteGuard {
            pointer,
            _gate: Some(gate),
            _root: Some(root),
        })
    }

    pub(crate) async fn read(&self) -> NativeLoanReadGuard {
        let mut wait = self.gate.read();
        std::future::poll_fn(|cx| {
            let root = self
                .pin()
                .expect("native Store loan is stale, retiring, or still executing");
            match Pin::new(&mut wait).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(gate) => {
                    let pointer = self.pointer(&root).expect("native Store loan scope ended");
                    Poll::Ready(NativeLoanReadGuard {
                        pointer,
                        _gate: Some(gate),
                        _root: Some(root),
                    })
                }
            }
        })
        .await
    }

    pub(crate) async fn write(&self) -> NativeLoanWriteGuard {
        let mut wait = self.gate.write();
        std::future::poll_fn(|cx| {
            let root = self
                .pin()
                .expect("native Store loan is stale, retiring, or still executing");
            match Pin::new(&mut wait).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(gate) => {
                    let pointer = self.pointer(&root).expect("native Store loan scope ended");
                    Poll::Ready(NativeLoanWriteGuard {
                        pointer,
                        _gate: Some(gate),
                        _root: Some(root),
                    })
                }
            }
        })
        .await
    }
}

pub(crate) fn native_coroutine_active() -> bool {
    CoroutineContext::current_root().is_some()
}

pub(crate) fn current_store_loan(store_id: StoreId) -> Option<crate::StoreAsync> {
    let weak = CoroutineContext::current_root()?;
    let root = weak.owner.upgrade()?;
    if root.store().id != store_id || root.control.error().is_some() || root.control.settled.get() {
        return None;
    }
    let (scope, epoch) = StoreContext::borrowed_loan_epoch(root.store_context_id, store_id)?;
    let gate = root.loan_gate.get_or_init(|| LocalRwLock::new(())).clone();
    Some(StoreAsync {
        native_invocation: root.store().native_invocation.clone(),
        id: store_id,
        inner: root.store().inner.clone(),
        loan: Some(NativeStoreLoan {
            root: weak,
            scope,
            epoch,
            gate,
            store_id,
        }),
    })
}

impl NativeRootOwner {
    fn store(&self) -> &StoreAsync {
        self.store
            .as_ref()
            .expect("native root Store already released")
    }

    fn has_coroutine(&self) -> bool {
        self.stack.borrow().coroutine.is_some()
    }
}

struct RootResume<'a> {
    root: &'a NativeRootOwner,
    returned: bool,
}

impl Drop for RootResume<'_> {
    fn drop(&mut self) {
        if !self.returned {
            // A panicked resume destroys the engine stack before propagating.
            // Do not admit a new pin while the invalid public future is retained.
            self.root.control.settle();
        }
    }
}

struct RetainedRootBoundary {
    control: Rc<NativeRootControl>,
    keep: bool,
}

impl Drop for RetainedRootBoundary {
    fn drop(&mut self) {
        if !self.keep {
            self.control.unregister();
        }
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
        let store_context_id = StoreContext::new_installation_id();
        let id = NEXT_ASYNC_CALL_FUTURE_ID
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |id| id.checked_add(1),
            )
            .expect("native future identity exhausted");
        let ancestors = store.loan.as_ref().map(|loan| {
            let parent = &loan.root.control;
            let mut ids = parent.ancestors.as_deref().unwrap_or(&[]).to_vec();
            ids.push(parent.future_id);
            Arc::<[AsyncCallFutureId]>::from(ids)
        });
        let control = Rc::new(NativeRootControl {
            future_id: id,
            scope: store
                .native_invocation
                .as_ref()
                .map(|generation| RootInvocationScope {
                    local: INVOCATION_SCOPES.with(|scopes| {
                        scopes
                            .borrow()
                            .get(&generation.id)
                            .cloned()
                            .unwrap_or_default()
                    }),
                    generation: generation.clone(),
                    started: Cell::new(false),
                }),
            ancestors,
            store_id,
            installation: store_context_id,
            retiring: Cell::new(false),
            settled: Cell::new(false),
            retained: Cell::new(false),
            parent: store
                .loan
                .as_ref()
                .map(|loan| Rc::downgrade(&loan.root.control)),
            native_edges: RefCell::new(Vec::new()),
            native_reason: OnceCell::new(),
            previous_row: RefCell::new(None),
            reason: OnceCell::new(),
            terminal: OnceCell::new(),
        });
        if let Some(scope) = &control.scope
            && scope.generation.owner == std::thread::current().id()
        {
            let local = INVOCATION_SCOPES.with(|scopes| {
                scopes
                    .borrow()
                    .get(&scope.generation.id)
                    .and_then(Weak::upgrade)
            });
            if let Some(local) = local {
                local
                    .controls
                    .borrow_mut()
                    .insert(id, Rc::downgrade(&control));
            }
        }
        let root = Rc::new_cyclic(|_owner| {
            let weak = WeakNativeRoot {
                owner: _owner.clone(),
                id: store_context_id,
                control: control.clone(),
            };
            let coroutine =
                Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                    assert!(matches!(resume, AsyncResume::Start));
                    let mut ctx_state = CoroutineContext::new(yielder, store_context_id);
                    ctx_state.owner = Some(weak);
                    ctx_state.enter();
                    let result = {
                        let mut store_mut = AsyncCallStoreMut { store_id };
                        function.call_native_root(&mut store_mut, &params, store_context_id)
                    };
                    ctx_state.leave();
                    result
                });
            NativeRootOwner {
                store: Some(store),
                store_context_id,
                control,
                parked: Cell::new(false),
                loan_gate: OnceCell::new(),
                frame_gate: OnceCell::new(),
                frame_resume_waiting: Cell::new(false),
                stack: RefCell::new(NativeRootStack {
                    coroutine: Some(coroutine),
                    suspended_stores: SuspendedStoreContexts::default(),
                    trap_state: RootTrapState::default(),
                }),
            }
        });
        Self {
            id,
            store_id,
            root: Some(root),
            pending_resume_gate: None,
            pending_frame_gate: None,
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
        }
    }

    fn root(&self) -> &Rc<NativeRootOwner> {
        self.root.as_ref().expect("native root already released")
    }

    fn store(&self) -> &StoreAsync {
        self.root().store()
    }

    fn remove_from_wakers_list(&self) {
        remove_from_wakers_list(self.store_id, self.id);
    }

    fn retire_if_needed(&mut self) -> Option<RuntimeError> {
        let error = self.root().control.error()?;
        self.retire_root(Some(error.clone()));
        Some(error)
    }
}

fn take_registered_waker(store_id: StoreId, id: AsyncCallFutureId) -> Option<RegisteredWaker> {
    let mut wakers_entry = match ASYNC_CALL_FUTURE_WAKERS.entry(store_id) {
        dashmap::Entry::Occupied(o) => o,
        dashmap::Entry::Vacant(_) => return None,
    };

    let mut waker_map_ref = wakers_entry.get_mut();
    let removed = waker_map_ref.remove(&id);
    if waker_map_ref.is_empty() {
        wakers_entry.remove();
    } else {
        drop(wakers_entry);
    }
    removed
}

fn remove_from_wakers_list(store_id: StoreId, id: AsyncCallFutureId) {
    // User Waker destructors may reenter this registry.
    drop(take_registered_waker(store_id, id));
}

fn register_waker(
    store_id: StoreId,
    id: AsyncCallFutureId,
    ancestors: Option<&Arc<[AsyncCallFutureId]>>,
    waker: &Waker,
) {
    // will_wake compares RawWaker identity without calling its vtable. A poll
    // with an unchanged Waker needs neither a user clone nor an allocation.
    let unchanged = ASYNC_CALL_FUTURE_WAKERS
        .get(&store_id)
        .and_then(|entry| entry.get(&id).map(|old| old.waker.will_wake(waker)))
        .unwrap_or(false);
    if unchanged {
        replace_registered_ancestors(store_id, id, ancestors.cloned());
        return;
    }
    let next = RegisteredWaker {
        waker: Arc::new(waker.clone()),
        ancestors: ancestors.cloned(),
    };
    let previous = {
        let mut entry = ASYNC_CALL_FUTURE_WAKERS.entry(store_id).or_default();
        entry.insert(id, next)
    };
    drop(previous);
}

fn replace_registered_ancestors(
    store_id: StoreId,
    id: AsyncCallFutureId,
    ancestors: Option<Arc<[AsyncCallFutureId]>>,
) {
    let old = {
        let Some(mut entry) = ASYNC_CALL_FUTURE_WAKERS.get_mut(&store_id) else {
            return;
        };
        let Some(record) = entry.get_mut(&id) else {
            return;
        };
        std::mem::replace(&mut record.ancestors, ancestors)
    };
    drop(old);
}

pub(crate) fn check_async_entry(store: &StoreAsync) -> Result<(), RuntimeError> {
    if let Some(error) = current_retirement_error() {
        return Err(error);
    }
    assert!(
        StoreContext::async_root_admissible(),
        "Function::call_async futures cannot be polled recursively from within another imported function"
    );
    if let Some(loan) = &store.loan {
        if let Some(error) = loan.root.control.error() {
            return Err(error);
        }
        if !loan.is_live() {
            return Err(RuntimeError::new("native child Store loan scope has ended"));
        }
    } else if RETIRING_ROOT.with(|current| {
        current.borrow().as_ref().is_some_and(|control| {
            control.store_id == store.id && control.retained.get() && !control.settled.get()
        })
    }) {
        return Err(RuntimeError::new(
            "a retained Store requires its explicit loan capability",
        ));
    }
    if let Some(generation) = &store.native_invocation {
        generation.check()?;
    }
    Ok(())
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let generation = self
            .root()
            .control
            .scope
            .as_ref()
            .map(|scope| scope.generation.clone());
        if generation.is_none() {
            return self.poll_inner(cx);
        }
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.poll_inner(cx)));
        notify_scope_after(generation, outcome)
    }
}

impl AsyncCallFuture {
    fn poll_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<Box<[Value]>, RuntimeError>> {
        // Existing exact root/ancestor abort takes precedence over a later close.
        if let Some(error) = self.retire_if_needed() {
            return Poll::Ready(Err(error));
        }
        if let Err(error) = check_async_entry(self.store()) {
            self.retire_root(Some(error.clone()));
            return Poll::Ready(Err(error));
        }
        if native_cleanup_no_yield()
            && matches!(self.next_resume.as_ref(), Some(AsyncResume::Start))
        {
            let error = RuntimeError::new(
                "cannot enter a new native async root during synchronous cleanup",
            );
            self.retire_root(None);
            return Poll::Ready(Err(error));
        }
        loop {
            if let Some(error) = self.root().control.error() {
                self.retire_root(Some(error.clone()));
                return Poll::Ready(Err(error));
            }
            let store_id = self.store().store_id();

            #[cfg(feature = "experimental-host-interrupt")]
            {
                if super::vm::interrupt_registry::is_interrupted(store_id) {
                    let error = super::vm::Trap::lib(super::vm::TrapCode::HostInterrupt).into();
                    self.retire_root(None);
                    return Poll::Ready(Err(error));
                }
            }

            if self.root().has_coroutine() {
                // The pending host future may return before the native stack
                // resumes. Interrupt notification still needs this poll's Waker.
                register_waker(
                    store_id,
                    self.id,
                    self.root().control.combined_ancestors().as_ref(),
                    cx.waker(),
                );
                // RawWaker clone/replacement callbacks can cancel an ancestor
                // or end an unstarted child's scope. Recheck before borrowing.
                if let Err(error) = check_async_entry(self.store()) {
                    self.retire_root(Some(error.clone()));
                    return Poll::Ready(Err(error));
                }
                // An Owned Store may still have a native ancestor on another
                // Store; that lineage is carried by this control, not the cap.
                if let Some(error) = self.retire_if_needed() {
                    return Poll::Ready(Err(error));
                }
            }

            let logical = RetirementBoundary::enter(&self.root().control);
            if let Some(future) = self.pending_future.as_mut() {
                let outcome = future.as_mut().poll(cx);
                if let Some(error) = self.root().control.error() {
                    if let Poll::Ready(result) = outcome {
                        assert!(self.next_resume.is_none());
                        self.next_resume = Some(AsyncResume::HostFutureReady(result));
                    }
                    self.retire_root(Some(error.clone()));
                    return Poll::Ready(Err(error));
                }
                match outcome {
                    Poll::Ready(result) => {
                        self.pending_future = None;
                        self.next_resume = Some(AsyncResume::HostFutureReady(result));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }
            drop(logical);
            if let Some(error) = self.retire_if_needed() {
                return Poll::Ready(Err(error));
            }

            // If we're ready, return early
            if !self.root().has_coroutine() {
                self.remove_from_wakers_list();
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            // Publish resume intent before any loan/frame/Store-lock callback.
            // A callback must not install a new lazy frame gate after we have
            // decided that no frame gate exists for this resume.
            self.root().frame_resume_waiting.set(true);

            // The established local lock owns all waiting/waking behavior. Its
            // write guard excludes every actual outside loan guard from resume.
            if self.pending_resume_gate.is_none()
                && let Some(gate) = self.root().loan_gate.get()
            {
                self.pending_resume_gate = Some(gate.write());
            }
            let resume_gate = if let Some(wait) = self.pending_resume_gate.as_mut() {
                match Pin::new(wait).poll(cx) {
                    Poll::Pending => {
                        return match self.retire_if_needed() {
                            Some(error) => Poll::Ready(Err(error)),
                            None => Poll::Pending,
                        };
                    }
                    Poll::Ready(guard) => {
                        self.pending_resume_gate = None;
                        Some(guard)
                    }
                }
            } else {
                None
            };
            if self.pending_frame_gate.is_none()
                && let Some(gate) = self.root().frame_gate.get()
            {
                self.pending_frame_gate = Some(gate.write());
            }
            let frame_gate = if let Some(wait) = self.pending_frame_gate.as_mut() {
                match Pin::new(wait).poll(cx) {
                    Poll::Pending => {
                        return match self.retire_if_needed() {
                            Some(error) => Poll::Ready(Err(error)),
                            None => Poll::Pending,
                        };
                    }
                    Poll::Ready(guard) => {
                        self.pending_frame_gate = None;
                        Some(guard)
                    }
                }
            } else {
                None
            };
            // Guard acquisition can run user Waker callbacks. Keep the exact
            // retained snapshot owned until cancellation selects its driver.
            if let Some(error) = self.retire_if_needed() {
                return Poll::Ready(Err(error));
            }
            let owns_root = self.root().stack.borrow().suspended_stores.owns_root();
            let mut store_context_guard = if owns_root {
                // This is the ORIGINAL write guard, never unlocked/reacquired.
                let root = self.root().clone();
                let mut stack = root.stack.borrow_mut();
                ForcedStoreInstallGuard::restore_borrowed_root(&mut stack.suspended_stores)
            } else {
                if self.pending_store_install.is_none() {
                    self.pending_store_install = Some(Box::pin(install_store_context(
                        self.store().store(),
                        self.root().store_context_id,
                    )));
                }
                match self
                    .pending_store_install
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(cx)
                {
                    Poll::Ready(guard) => {
                        self.pending_store_install = None;
                        guard
                    }
                    Poll::Pending => {
                        return match self.retire_if_needed() {
                            Some(error) => Poll::Ready(Err(error)),
                            None => Poll::Pending,
                        };
                    }
                }
            };
            if let Err(error) = self.root().control.start_scope() {
                // A first-entry Store lock acquisition can run a Waker which closes
                // the generation. No engine stack has entered under this guard.
                assert!(!owns_root);
                drop(store_context_guard);
                self.retire_root(Some(error.clone()));
                return Poll::Ready(Err(error));
            }
            self.root().parked.set(false);
            self.root().frame_resume_waiting.set(false);
            // Drop after the stack borrow, but before its original write guard
            // unlocks and wakes a fresh invocation on this same Store.
            let mut retirement = RetainedRootBoundary {
                control: self.root().control.clone(),
                keep: false,
            };

            {
                // This strong reference lives on the executor stack, never on
                // the coroutine it owns. Only the stack fields stay borrowed
                // across resume; identity and retirement remain accessible.
                let root = self.root().clone();
                let mut stack = root.stack.borrow_mut();
                if !owns_root {
                    store_context_guard.restore_suffix(&mut stack.suspended_stores);
                }
                let resume_arg = self.next_resume.take().expect("no resume arg available");
                let trap_boundary = TrapBoundary::enter(std::mem::take(&mut stack.trap_state));
                let coroutine = stack.coroutine.as_mut().unwrap();
                let boundary = CoroutineBoundary::new();
                let mut resume = RootResume {
                    root: &root,
                    returned: false,
                };
                let result = coroutine.resume(resume_arg);
                boundary.returned();
                resume.returned = true;
                let trap_state = trap_boundary.leave();
                match result {
                    CoroutineResult::Yield(AsyncYield(fut)) => {
                        stack.trap_state = trap_state;
                        if StoreContext::can_suspend_borrowed_root(root.store_context_id) {
                            store_context_guard.suspend_borrowed_root(&mut stack.suspended_stores);
                            root.control.register(&stack.suspended_stores);
                            retirement.keep = true;
                        } else {
                            store_context_guard.suspend_suffix(&mut stack.suspended_stores);
                        }
                        self.pending_future = Some(fut);
                        root.parked.set(true);
                    }
                    CoroutineResult::Return(result) => {
                        stack.coroutine = None;
                        root.control.settle();
                        self.result = Some(result);
                    }
                }
            }

            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(retirement);
            drop(store_context_guard);
            drop(frame_gate);
            drop(resume_gate);
        }
    }
}

impl AsyncCallFuture {
    fn retire_root(&mut self, managed_reason: Option<RuntimeError>) {
        let Some(root) = self.root.take() else {
            return;
        };
        if !root.control.settled.get() {
            root.control.retire(managed_reason);
        }
        // Release Future custody even when a terminal Future is retained. Only
        // actual escaped loan guards may keep this stack and its parent alive.
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        let _retirement = RetirementBoundary::enter(&root.control);
        let root_teardown = RootTeardown::new(root);
        let owned = DroppedHostState {
            _pending_future: self.pending_future.take(),
            _pending_resume_gate: self.pending_resume_gate.take(),
            _pending_frame_gate: self.pending_frame_gate.take(),
            _pending_store_install: self.pending_store_install.take(),
            _next_resume: self.next_resume.take(),
            _result: self.result.take(),
        };
        let own_waker = take_registered_waker(self.store_id, self.id);
        let mut failures = if !root_teardown.root.control.settled.get() {
            notify_descendant_futures(self.id)
        } else {
            Vec::new()
        };
        defer_cleanup(&mut failures, || drop(own_waker));
        defer_cleanup(&mut failures, || drop(owned));
        defer_cleanup(&mut failures, || drop(root_teardown));
        resume_cleanup_panics(failures);
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        self.retire_root(None);
    }
}

// Keep the root alive while dropping the external host body, and return its
// exact trap state before releasing that strong reference. An outside pin can
// defer final stack destruction to a later, unrelated physical context.
struct RootTeardown {
    root: Rc<NativeRootOwner>,
    boundary: Option<TrapBoundary>,
}

impl RootTeardown {
    fn new(root: Rc<NativeRootOwner>) -> Self {
        let state = std::mem::take(&mut root.stack.borrow_mut().trap_state);
        Self {
            root,
            boundary: Some(TrapBoundary::enter(state)),
        }
    }
}

impl Drop for RootTeardown {
    fn drop(&mut self) {
        let state = self.boundary.take().expect("active root teardown").leave();
        self.root.stack.borrow_mut().trap_state = state;
        // The root field drops after this method, once its saved state is back.
    }
}

impl Drop for NativeRootOwner {
    fn drop(&mut self) {
        let generation = self
            .control
            .scope
            .as_ref()
            .map(|scope| scope.generation.clone());
        if generation.is_none() {
            return self.drop_stack();
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.drop_stack()));
        notify_scope_after(generation, outcome);
    }
}

impl NativeRootOwner {
    fn drop_stack(&mut self) {
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        let _retirement = RetirementBoundary::enter(&self.control);
        let stack = self.stack.get_mut();
        let _traps = TrapBoundary::enter(std::mem::take(&mut stack.trap_state));
        // Lock waiters may own user Wakers. Empty the field here so their last
        // destructor cannot escape the detached retirement/VM boundaries.
        let loan_gate = self.loan_gate.take();
        let frame_gate = self.frame_gate.take();
        let mut owned = DroppedNativeStack {
            _coroutine: stack.coroutine.take(),
            _suspended_stores: std::mem::take(&mut stack.suspended_stores),
            _store: self.store.take(),
        };
        let mut failures = Vec::new();
        if owned._suspended_stores.owns_root() {
            let reason = self
                .control
                .error()
                .expect("borrowed root retires before its final owner");
            let installed =
                ForcedStoreInstallGuard::restore_borrowed_root(&mut owned._suspended_stores);
            let mut retiring = RetiringNativeStack {
                coroutine: owned._coroutine.take(),
                yielded: Vec::new(),
                result: None,
                failures: Vec::new(),
                _settled: SettledNativeRoot(self.control.clone()),
                _retirement: RetainedRootBoundary {
                    control: self.control.clone(),
                    keep: false,
                },
                _installed: installed,
            };
            // Only actual outside guards pin the owner; none remain here.
            // Keep every yielded/returned payload until the stack is dead. A
            // destructor panic must never unlock beneath a suspended frame.
            loop {
                let mut outcome = None;
                let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let boundary = CoroutineBoundary::new();
                    outcome = Some(
                        retiring
                            .coroutine
                            .as_mut()
                            .expect("borrowed coroutine retained")
                            .resume(AsyncResume::HostFutureReady(Err(reason.clone()))),
                    );
                    boundary.returned();
                }));
                if let Err(panic) = resumed {
                    retiring.failures.push(panic);
                }
                if let Some(outcome) = outcome {
                    match outcome {
                        CoroutineResult::Return(result) => retiring.result = Some(result),
                        CoroutineResult::Yield(AsyncYield(future)) => retiring.yielded.push(future),
                    }
                }
                if retiring
                    .coroutine
                    .as_ref()
                    .expect("retiring coroutine retained")
                    .done()
                {
                    break;
                }
            }
            defer_cleanup(&mut retiring.failures, || drop(retiring.coroutine.take()));
            defer_cleanup(&mut retiring.failures, || drop(retiring.result.take()));
            while !retiring.yielded.is_empty() {
                let future = retiring.yielded.remove(0);
                // Never poll or replay an unexpected yield during retirement.
                defer_cleanup(&mut retiring.failures, || drop(future));
            }
            // Asyncify's callback belongs to this finished invocation. Its
            // resources remain owned until the old stack is dead, and it can
            // neither execute nor carry into a fresh root after cancellation.
            defer_cleanup(&mut retiring.failures, || {
                let callback = {
                    let mut wrapper = unsafe { StoreContext::get_current(self.control.store_id) };
                    wrapper.as_mut().inner.on_called.take()
                };
                drop(callback);
            });
            failures = std::mem::take(&mut retiring.failures);
            // The sentinel marks settled, then the exact weak row is removed,
            // then the original guard unlocks. Catch any unlock/Waker panic too.
            defer_cleanup(&mut failures, || drop(retiring));
        }
        defer_cleanup(&mut failures, || drop(owned));
        self.control.settle();
        defer_cleanup(&mut failures, || drop(loan_gate));
        defer_cleanup(&mut failures, || drop(frame_gate));
        resume_cleanup_panics(failures);
    }
}

type DeferredPanic = Box<dyn std::any::Any + Send>;

fn defer_cleanup(failures: &mut Vec<DeferredPanic>, cleanup: impl FnOnce()) {
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cleanup)) {
        failures.push(panic);
    }
}

// Safe guard disposal also runs while another Rust failure is unwinding.
// Settle every owned phase first, then preserve that already-active failure.
fn resume_guard_cleanup_panics(failures: Vec<DeferredPanic>, already_unwinding: bool) {
    if already_unwinding {
        for payload in failures {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)));
        }
    } else {
        resume_cleanup_panics(failures);
    }
}

fn resume_cleanup_panics(mut failures: Vec<DeferredPanic>) {
    if !failures.is_empty() {
        let first = failures.remove(0);
        // All owned cleanup has completed before panic payload disposal. An
        // actual escaped guard still owns its stack until its final driver.
        // Repeatedly panicking payload destructors have ordinary Rust behavior;
        // no payload is intentionally leaked.
        for payload in failures {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)));
        }
        std::panic::resume_unwind(first);
    }
}

// Field order is also the unwind fallback: dispose the real stack before any
// owned user payload, mark it settled, remove retirement metadata, then unlock.
#[allow(clippy::type_complexity)]
struct RetiringNativeStack {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    yielded: Vec<HostFuture>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,
    failures: Vec<DeferredPanic>,
    _settled: SettledNativeRoot,
    _retirement: RetainedRootBoundary,
    _installed: ForcedStoreInstallGuard,
}

struct SettledNativeRoot(Rc<NativeRootControl>);

impl Drop for SettledNativeRoot {
    fn drop(&mut self) {
        self.0.settle();
    }
}

struct DroppedHostState {
    _pending_future: Option<HostFuture>,
    _pending_resume_gate: Option<crate::WriteFuture<()>>,
    _pending_frame_gate: Option<crate::WriteFuture<()>>,
    _pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    _next_resume: Option<AsyncResume>,
    _result: Option<Result<Box<[Value]>, RuntimeError>>,
}

// Fields also drop in this order if a destructor panics. The stack's Store and
// raw Sync snapshots must outlive every native frame during its forced unwind.
#[allow(clippy::type_complexity)]
struct DroppedNativeStack {
    _coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    _suspended_stores: SuspendedStoreContexts,
    _store: Option<StoreAsync>,
}

// Baremetal has no trap-handler pointers, but still exchanges numeric VM scopes.
#[derive(Default)]
struct RootTrapState {
    state: wasmer_vm::SavedTrapState,
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    interrupts: wasmer_vm::interrupt_registry::SavedInterruptState,
}

impl RootTrapState {
    unsafe fn replace_current(self) -> Self {
        // No guest code or user callback runs between the interrupt marker and
        // VM handler exchanges. A signal during this host-stack transition is
        // rejected by the handler's Wasm-stack bounds check.
        Self {
            #[cfg(all(unix, feature = "experimental-host-interrupt"))]
            interrupts: unsafe { self.interrupts.replace_current() },
            state: unsafe { self.state.replace_current() },
        }
    }
}

struct TrapBoundary {
    caller: Option<RootTrapState>,
}

impl TrapBoundary {
    fn enter(root: RootTrapState) -> Self {
        // The invocation retains the coroutine and its Store until this
        // boundary is removed, including while its destructors unwind.
        Self {
            caller: Some(unsafe { root.replace_current() }),
        }
    }

    fn leave(mut self) -> RootTrapState {
        // Restore the caller before polling any host future. Only a normal
        // yield retains the returned raw pointers; Return discards them.
        unsafe {
            self.caller
                .take()
                .expect("active trap boundary")
                .replace_current()
        }
    }
}

impl Drop for TrapBoundary {
    fn drop(&mut self) {
        // On panic the engine stack may already be dead. Exchange never
        // dereferences its old pointers, and no token retains them afterward.
        if let Some(caller) = self.caller.take() {
            unsafe { caller.replace_current() };
        }
    }
}

async fn install_store_context(
    store: StoreAsync,
    installation: StoreContextId,
) -> ForcedStoreInstallGuard {
    match unsafe { crate::StoreContext::try_get_current_async(store.id) } {
        crate::GetStoreAsyncGuardResult::NotInstalled => {
            if let Some(loan) = &store.loan {
                let guard = loan.write().await;
                return StoreContext::install_loan(guard, installation, store.id);
            }
            let store_guard = store.inner.write().await;
            StoreContext::install_async_with_id(store_guard, installation)
        }
        _ => {
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

pub(crate) fn block_on_host_future<Fut>(future: Fut) -> Result<Vec<Value>, AsyncRuntimeError>
where
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
{
    match CoroutineContext::get_current() {
        None => {
            // If there is no async context or we haven't entered it,
            // we can still directly run a future that doesn't block
            // inline.
            run_immediate(future)
        }
        Some(context) => unsafe { context.as_ref().expect("valid context pointer") }
            .block_on_future(Box::pin(future))
            .map_err(AsyncRuntimeError::RuntimeError),
    }
}

// This function will only *wake* pending futures belonging to the given store.
// It is expected that the interrupt happens before this function is called, so
// that the futures can catch and react to `interrupt_registry::is_interrupted`
// correctly.
pub(crate) fn notify_pending_futures_of_interrupt(store_id: StoreId) {
    let wakers: Vec<_> = {
        let Some(entry) = ASYNC_CALL_FUTURE_WAKERS.get(&store_id) else {
            return;
        };
        // Only Arc cloning happens under the entry. RawWaker clone, wake and
        // Drop can run arbitrary code that reenters this same Store's registry.
        entry.values().map(|record| record.waker.clone()).collect()
    };
    for waker in wakers {
        waker.wake_by_ref();
    }
}

fn notify_descendant_futures(ancestor: AsyncCallFutureId) -> Vec<DeferredPanic> {
    let wakers: Vec<_> = {
        let mut wakers = Vec::new();
        for entry in ASYNC_CALL_FUTURE_WAKERS.iter() {
            wakers.extend(
                entry
                    .values()
                    .filter(|record| {
                        record
                            .ancestors
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&ancestor))
                    })
                    .map(|record| record.waker.clone()),
            );
        }
        wakers
    };
    let mut failures = Vec::new();
    {
        let _logical = DetachedRootMetadata::new();
        for waker in &wakers {
            defer_cleanup(&mut failures, || waker.wake_by_ref());
        }
    }
    // Snapshot/removed Waker destructors retain the cancellation metadata.
    for waker in wakers {
        defer_cleanup(&mut failures, || drop(waker));
    }
    failures
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<*const CoroutineContext>> = const { RefCell::new(Vec::new()) };
    // This synchronous physical-scope restriction survives metadata detachment
    // around arbitrary Waker callbacks. It owns no Store or coroutine authority.
    static NATIVE_CLEANUP_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
    store_context_id: StoreContextId,
    owner: Option<WeakNativeRoot>,
}

/// Hide a foreign physical coroutine from all cancellation destructors.
struct DetachedCoroutineContext {
    caller: Vec<*const CoroutineContext>,
}

impl DetachedCoroutineContext {
    fn new() -> Self {
        Self {
            caller: CURRENT_CONTEXT.with(|current| std::mem::take(&mut *current.borrow_mut())),
        }
    }
}

impl Drop for DetachedCoroutineContext {
    fn drop(&mut self) {
        let remaining = CURRENT_CONTEXT.with(|current| {
            // Any destructor-created coroutine is owned by that destructor;
            // none may outlive it or replace the caller's exact prefix.
            std::mem::replace(&mut *current.borrow_mut(), std::mem::take(&mut self.caller))
        });
        if !std::thread::panicking() {
            assert!(
                remaining.is_empty(),
                "destructor left a coroutine installed"
            );
        }
    }
}

/// Raw context pointers belong to coroutine stacks. Unwinding a resume destroys
/// those stacks before propagating the panic, so remove only that resume's suffix.
struct CoroutineBoundary {
    prefix: Vec<*const CoroutineContext>,
    returned: bool,
}

impl CoroutineBoundary {
    fn new() -> Self {
        Self {
            prefix: CURRENT_CONTEXT.with(|current| current.borrow().clone()),
            returned: false,
        }
    }

    fn returned(mut self) {
        CURRENT_CONTEXT.with(|current| {
            assert_eq!(
                *current.borrow(),
                self.prefix,
                "coroutine did not restore its context before yielding or returning"
            );
        });
        self.returned = true;
    }
}

impl Drop for CoroutineBoundary {
    fn drop(&mut self) {
        if !self.returned {
            CURRENT_CONTEXT.with(|current| {
                let mut contexts = current.borrow_mut();
                // Never discard a caller whose ancestry has been disturbed by
                // another invariant failure during unwinding.
                if contexts.starts_with(&self.prefix) {
                    contexts.truncate(self.prefix.len());
                }
            });
        }
    }
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>, store_context_id: StoreContextId) -> Self {
        Self {
            yielder: yielder as *const _,
            store_context_id,
            owner: None,
        }
    }

    fn enter(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Push this coroutine on top of the active stack.
            borrow.push(self as *const _);
        })
    }

    // The owning resume boundary handles panic cleanup. A stack-local guard
    // must not pop this context again while corosensei unwinds its stack.
    fn leave(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Pop this coroutine from the active stack.
            assert_eq!(
                borrow.last().copied(),
                Some(self as *const _),
                "Active coroutine stack corrupted"
            );
            borrow.pop();
        });
    }

    fn get_current() -> Option<*const Self> {
        CURRENT_CONTEXT.with(|cell| cell.borrow().last().copied())
    }

    fn current_root() -> Option<WeakNativeRoot> {
        let current = Self::get_current()?;
        unsafe { &*current }.owner.clone()
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        if let Some(error) = self.owner.as_ref().and_then(|root| root.control.error()) {
            return Err(error);
        }
        let cleanup = native_cleanup_no_yield();
        if cleanup
            || (!StoreContext::can_suspend_suffix(self.store_context_id)
                && !StoreContext::can_suspend_borrowed_root(self.store_context_id))
        {
            return run_immediate(future).map_err(|error| match error {
                AsyncRuntimeError::RuntimeError(error) => error,
                AsyncRuntimeError::YieldOutsideAsyncContext => RuntimeError::new(if cleanup {
                    "cannot suspend during synchronous native cleanup"
                } else {
                    "cannot suspend a native call while a borrowed or incompatible Store context remains installed"
                }),
            });
        }
        // Leave the coroutine context since we're yielding back to the
        // parent stack, and will be inactive until the future is ready.
        self.leave();

        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        let result = match yielder.suspend(AsyncYield(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        };

        // Once the future is ready, we restore the current coroutine
        // context.
        self.enter();

        result
    }
}

fn run_immediate(
    future: impl Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
) -> Result<Vec<Value>, AsyncRuntimeError> {
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.map_err(AsyncRuntimeError::RuntimeError),
        Poll::Pending => Err(AsyncRuntimeError::YieldOutsideAsyncContext),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, Instance, Module};

    #[test]
    fn external_cleanup_can_be_polled_inside_a_native_descendant() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::external_cleanup_can_be_polled_inside_a_native_descendant",
        ) {
            return;
        }
        type Cleanup = Pin<Box<dyn Future<Output = NativeInvocationScope>>>;
        thread_local! {
            static ANCESTOR: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static CALL: RefCell<Option<Pin<Box<AsyncCallFuture>>>> = const { RefCell::new(None) };
            static CLEANUP: RefCell<Option<Cleanup>> = const { RefCell::new(None) };
            static ERROR: RefCell<Option<RuntimeError>> = const { RefCell::new(None) };
        }
        let mut first_store = Store::default();
        let pending = Function::new_typed(&mut first_store, || -> Result<i32, RuntimeError> {
            ANCESTOR.with(|slot| *slot.borrow_mut() = WeakNativeInvocation::current());
            block_on_host_future(std::future::pending()).map_err(|error| match error {
                AsyncRuntimeError::RuntimeError(error) => error,
                _ => RuntimeError::new("scope cleanup proof failed to suspend"),
            })?;
            Ok(42)
        });
        let fresh = Function::new_typed(&mut first_store, || -> i32 { 42 });
        let scope = NativeInvocationScope::bind(&mut first_store).unwrap();
        let first_store = first_store.into_async();
        let mut call = Box::pin(AsyncCallFuture::new(
            pending.into_sys(),
            first_store.store(),
            vec![],
        ));
        let noop = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        CALL.with(|slot| *slot.borrow_mut() = Some(call));
        CLEANUP.with(|slot| {
            *slot.borrow_mut() = Some(Box::pin(async move {
                let mut scope = scope;
                // This first poll occurs inline under B. The public waiter
                // still refuses awaiting its own native ancestor lifetime.
                assert!(scope.close_and_wait().await.is_err());
                scope.close_and_wait_for_cleanup().await;
                scope
            }));
        });
        let mut second_store = Store::default();
        let descendant = Function::new_typed(&mut second_store, || -> i32 {
            let ancestor = ANCESTOR.with(|slot| slot.borrow_mut().take().unwrap());
            // A pure Rust model of the external native scope: scope exit below
            // precedes guard release and any deferred ancestor stack retirement.
            let frame = unsafe { ancestor.enter_descendant_frame() }.unwrap();
            let before = CoroutineContext::get_current().unwrap();
            drop(CALL.with(|slot| slot.borrow_mut().take().unwrap()));
            ERROR.with(|slot| *slot.borrow_mut() = ancestor.retirement_error());
            CLEANUP.with(|slot| {
                let mut cleanup = slot.borrow_mut();
                let noop = futures::task::noop_waker();
                assert!(
                    cleanup
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut Context::from_waker(&noop))
                        .is_pending()
                );
            });
            assert_eq!(CoroutineContext::get_current(), Some(before));
            // The modeled external isolate entry has exited. Only now may its
            // frame guard release A's last stack pin and complete retirement.
            drop(frame);
            assert_eq!(CoroutineContext::get_current(), Some(before));
            42
        });
        let error =
            futures::executor::block_on(descendant.call_async(&second_store.into_async(), vec![]))
                .unwrap_err();
        let expected = ERROR.with(|slot| slot.borrow_mut().take().unwrap());
        assert!(Arc::ptr_eq(&error.inner, &expected.inner));
        let mut cleanup = CLEANUP.with(|slot| slot.borrow_mut().take().unwrap());
        let Poll::Ready(scope) = cleanup.as_mut().poll(&mut Context::from_waker(&noop)) else {
            panic!("external cleanup did not observe ancestor settlement");
        };
        drop(cleanup);
        assert_eq!(
            scope
                .local
                .generation
                .active
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        let (next_scope, next_store) = {
            let mut write = first_store.store().write();
            scope
                .successor(&first_store, &mut write.as_store_mut())
                .unwrap()
        };
        assert_eq!(
            futures::executor::block_on(fresh.call_async(&next_store, vec![])).unwrap()[0].i32(),
            Some(42)
        );
        drop(next_scope);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn resume_intent_precedes_a_descendant_frame_created_by_a_store_waiter() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::resume_intent_precedes_a_descendant_frame_created_by_a_store_waiter",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        type RootCall = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;
        thread_local! {
            static ANCESTOR: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static LOCKED: RefCell<Option<crate::StoreAsyncWriteLock>> = const { RefCell::new(None) };
            static ON_READY: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
            static CHILD: RefCell<Option<RootCall>> = const { RefCell::new(None) };
        }
        struct CloneState {
            body_ready: Arc<AtomicBool>,
        }
        impl CloneState {
            fn waker(self: &Arc<Self>) -> Waker {
                unsafe fn clone(data: *const ()) -> RawWaker {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<CloneState>())
                    });
                    if state.body_ready.load(Ordering::SeqCst) {
                        let callback = ON_READY.with(|slot| slot.borrow_mut().take());
                        if let Some(callback) = callback {
                            callback();
                        }
                    }
                    RawWaker::new(Arc::into_raw(Arc::clone(&state)).cast(), &VTABLE)
                }
                unsafe fn wake(data: *const ()) {
                    drop(unsafe { Arc::from_raw(data.cast::<CloneState>()) });
                }
                unsafe fn wake_by_ref(_: *const ()) {}
                unsafe fn drop_waker(data: *const ()) {
                    drop(unsafe { Arc::from_raw(data.cast::<CloneState>()) });
                }
                static VTABLE: RawWakerVTable =
                    RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);
                unsafe {
                    Waker::from_raw(RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE))
                }
            }
        }
        struct ExternalScope(Arc<AtomicBool>);
        impl Drop for ExternalScope {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        struct Descendant {
            leaf: Option<crate::TypedFunction<(), i32>>,
            active: Arc<AtomicBool>,
            denied: Arc<AtomicBool>,
        }
        let mut store = Store::default();
        let ready = Arc::new(AtomicBool::new(false));
        let body_ready = Arc::new(AtomicBool::new(false));
        let descendant_active = Arc::new(AtomicBool::new(false));
        let resumed_above_child = Arc::new(AtomicBool::new(false));
        let parent_ready = ready.clone();
        let parent_body = body_ready.clone();
        let leaf = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            move |_| {
                ANCESTOR.with(|slot| {
                    assert!(
                        slot.borrow_mut()
                            .replace(WeakNativeInvocation::current().unwrap())
                            .is_none()
                    )
                });
                let ready = parent_ready.clone();
                let observed = parent_body.clone();
                async move {
                    std::future::poll_fn(|_| {
                        if ready.load(Ordering::SeqCst) {
                            observed.store(true, Ordering::SeqCst);
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    Ok(vec![Value::I32(42)])
                }
            },
        );
        let child_active = descendant_active.clone();
        let premature = resumed_above_child.clone();
        let after = Function::new_typed(&mut store, move || {
            premature.store(child_active.load(Ordering::SeqCst), Ordering::SeqCst);
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "leaf" (func $leaf (result i32)))
            (import "proof" "after" (func $after))
            (func (export "run") (result i32) call $leaf call $after)
            (func (export "fresh") (result i32) i32.const 42))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "leaf" => leaf, "after" => after }},
        )
        .unwrap();
        let store = store.into_async();
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let noop = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        let token = ANCESTOR.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert!(
            token
                .root
                .owner
                .upgrade()
                .unwrap()
                .frame_gate
                .get()
                .is_none()
        );
        assert!(
            !token
                .root
                .owner
                .upgrade()
                .unwrap()
                .stack
                .borrow()
                .suspended_stores
                .owns_root()
        );

        let mut other = Store::default();
        let denied = Arc::new(AtomicBool::new(false));
        let state = crate::FunctionEnv::new(
            &mut other,
            Descendant {
                leaf: None,
                active: descendant_active.clone(),
                denied: denied.clone(),
            },
        );
        let wait = Function::new_async(
            &mut other,
            crate::FunctionType::new([], [crate::Type::I32]),
            |_| std::future::pending::<Result<Vec<Value>, RuntimeError>>(),
        );
        let bridge = Function::new_typed_with_env(
            &mut other,
            &state,
            |mut env: crate::FunctionEnvMut<Descendant>| -> Result<i32, RuntimeError> {
                let ancestor = ANCESTOR.with(|slot| slot.borrow().as_ref().unwrap().clone());
                let frame = match unsafe { ancestor.enter_descendant_frame() } {
                    Ok(frame) => frame,
                    Err(_) => {
                        env.data().denied.store(true, Ordering::SeqCst);
                        return Ok(7);
                    }
                };
                let (data, mut derived) = env.data_and_store_mut();
                data.active.store(true, Ordering::SeqCst);
                let external = ExternalScope(data.active.clone());
                let result = data.leaf.as_ref().unwrap().call(&mut derived);
                drop(external);
                drop(frame);
                result
            },
        );
        let module = Module::new(
            &other,
            r#"(module
            (import "proof" "wait" (func $wait (result i32)))
            (import "proof" "bridge" (func $bridge (result i32)))
            (func (export "run") (result i32) call $bridge)
            (func (export "leaf") (result i32) call $wait))"#,
        )
        .unwrap();
        let child_instance = Instance::new(
            &mut other,
            &module,
            &crate::imports! { "proof" => { "wait" => wait, "bridge" => bridge }},
        )
        .unwrap();
        state.as_mut(&mut other).leaf = Some(
            child_instance
                .exports
                .get_typed_function(&other, "leaf")
                .unwrap(),
        );
        let other = other.into_async();
        let child_store = other.store();
        let child_function = child_instance.exports.get_function("run").unwrap().clone();
        let callbacks = Arc::new(AtomicUsize::new(0));
        let fired = callbacks.clone();
        LOCKED.with(|slot| *slot.borrow_mut() = Some(store.store().write()));
        ON_READY.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                fired.fetch_add(1, Ordering::SeqCst);
                let mut child: RootCall = Box::pin(child_function.call_async(&child_store, vec![]));
                let noop = futures::task::noop_waker();
                match child.as_mut().poll(&mut Context::from_waker(&noop)) {
                    Poll::Pending => {
                        CHILD.with(|slot| assert!(slot.borrow_mut().replace(child).is_none()))
                    }
                    Poll::Ready(Ok(values)) => assert_eq!(values[0].i32(), Some(7)),
                    Poll::Ready(Err(error)) => {
                        panic!("child failed outside frame admission: {error}")
                    }
                }
                // This legitimate clone callback makes the original Store lock
                // ready after a different physical root attempted frame entry.
                drop(LOCKED.with(|slot| slot.borrow_mut().take()));
            }));
        });
        let state = Arc::new(CloneState { body_ready });
        let waker = state.waker();
        ready.store(true, Ordering::SeqCst);
        let outcome = call.as_mut().poll(&mut Context::from_waker(&waker));
        let completed =
            matches!(outcome, Poll::Ready(Ok(ref values)) if values[0].i32() == Some(42));
        // Every modeled native scope and actual Rust stack settles before any
        // discriminator, including the old implementation's unsafe ordering.
        drop(LOCKED.with(|slot| slot.borrow_mut().take()));
        drop(call);
        drop(CHILD.with(|slot| slot.borrow_mut().take()));
        assert!(!descendant_active.load(Ordering::SeqCst));
        assert!(completed);
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        assert!(
            !resumed_above_child.load(Ordering::SeqCst),
            "ancestor resumed above a descendant frame installed by a lock callback"
        );
        assert!(
            denied.load(Ordering::SeqCst),
            "resume intent did not reject the new native descendant"
        );
        assert_eq!(
            futures::executor::block_on(
                instance
                    .exports
                    .get_function("fresh")
                    .unwrap()
                    .call_async(&store, vec![])
            )
            .unwrap()[0]
                .i32(),
            Some(42)
        );
        ANCESTOR.with(|slot| drop(slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        assert!(CoroutineContext::current_root().is_none());
    }

    #[test]
    fn host_cleanup_uses_enclosing_poll_custody_when_the_resume_writer_is_held() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::host_cleanup_uses_enclosing_poll_custody_when_the_resume_writer_is_held",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        thread_local! {
            static TOKEN: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static ON_WRITER: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
            static CLOSED: RefCell<Option<NativeInvocationScope>> = const { RefCell::new(None) };
        }
        fn maybe_enter_cleanup() {
            let token = TOKEN.with(|slot| slot.borrow().as_ref().unwrap().clone());
            let Some(owner) = token.root.owner.upgrade() else {
                return;
            };
            let held = owner.parked.get()
                && owner.frame_resume_waiting.get()
                && owner.frame_gate.get().unwrap().try_read().is_none();
            drop(owner);
            if held {
                let callback = ON_WRITER.with(|slot| slot.borrow_mut().take());
                if let Some(callback) = callback {
                    callback();
                }
            }
        }
        struct WakeState {
            cleanup_wakes: AtomicUsize,
        }
        impl WakeState {
            fn waker(self: &Arc<Self>) -> Waker {
                unsafe fn clone(data: *const ()) -> RawWaker {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<WakeState>())
                    });
                    maybe_enter_cleanup();
                    RawWaker::new(Arc::into_raw(Arc::clone(&state)).cast(), &VTABLE)
                }
                unsafe fn wake(data: *const ()) {
                    unsafe { wake_by_ref(data) };
                    drop(unsafe { Arc::from_raw(data.cast::<WakeState>()) });
                }
                unsafe fn wake_by_ref(data: *const ()) {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<WakeState>())
                    });
                    if native_cleanup_no_yield() {
                        state.cleanup_wakes.fetch_add(1, Ordering::SeqCst);
                    }
                }
                unsafe fn drop_waker(data: *const ()) {
                    drop(unsafe { Arc::from_raw(data.cast::<WakeState>()) });
                }
                static VTABLE: RawWakerVTable =
                    RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);
                // Every raw value owns one Arc; clone/by_ref borrow their
                // original, while wake/drop consume only their own Arc.
                unsafe {
                    Waker::from_raw(RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE))
                }
            }
        }
        let mut store = Store::default();
        let ready = Arc::new(AtomicBool::new(false));
        let body_ready = ready.clone();
        let leaf = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            move |_| {
                TOKEN.with(|slot| {
                    assert!(
                        slot.borrow_mut()
                            .replace(WeakNativeInvocation::current().unwrap())
                            .is_none()
                    )
                });
                let ready = body_ready.clone();
                async move {
                    std::future::poll_fn(|_| {
                        if ready.load(Ordering::SeqCst) {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    Ok(vec![Value::I32(42)])
                }
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "leaf" (func $leaf (result i32)))
            (func (export "run") (result i32) call $leaf)
            (func (export "fresh") (result i32) i32.const 42))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "leaf" => leaf }},
        )
        .unwrap();
        let scope = NativeInvocationScope::bind(&mut store).unwrap();
        let store = store.into_async();
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let noop = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        let token = TOKEN.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert!(
            !token
                .root
                .owner
                .upgrade()
                .unwrap()
                .stack
                .borrow()
                .suspended_stores
                .owns_root()
        );
        let frame = unsafe { token.enter_descendant_frame() }.unwrap();
        drop(frame);
        let locked = store.store().write();
        let entered = Arc::new(AtomicUsize::new(0));
        let entry = entered.clone();
        let header_exited = Arc::new(AtomicBool::new(false));
        let exited = header_exited.clone();
        let ancestor = token.clone();
        ON_WRITER.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let cleanup = unsafe { NativeCleanupGuard::enter(Some(&ancestor)) }.unwrap();
                assert!(cleanup.root.is_some() && cleanup.gate.is_none());
                assert!(native_cleanup_no_yield());
                assert_eq!(scope.local.generation.active.load(Ordering::SeqCst), 1);
                scope.close();
                assert!(ancestor.retirement_error().is_some());
                assert!(!ancestor.root.control.settled.get());
                entry.fetch_add(1, Ordering::SeqCst);
                // The enclosing poll cannot resume until this synchronous
                // callback exits and restores its modeled engine header.
                exited.store(true, Ordering::SeqCst);
                drop(cleanup);
                assert!(!native_cleanup_no_yield());
                assert!(!ancestor.root.control.settled.get());
                CLOSED.with(|slot| assert!(slot.borrow_mut().replace(scope).is_none()));
            }));
        });
        let state = Arc::new(WakeState {
            cleanup_wakes: AtomicUsize::new(0),
        });
        let waker = state.waker();
        ready.store(true, Ordering::SeqCst);
        let outcome = call.as_mut().poll(&mut Context::from_waker(&waker));
        // A defective admission path still has only Rust stacks here. Release
        // its Store guard and cancel before checking the modeled-scope result.
        drop(locked);
        let rejected = matches!(outcome, Poll::Ready(Err(ref error)) if error.is_host_abort());
        drop(call);
        assert!(
            rejected,
            "closed invocation resumed through its cleanup frame"
        );
        assert_eq!(entered.load(Ordering::SeqCst), 1);
        assert!(header_exited.load(Ordering::SeqCst));
        assert!(state.cleanup_wakes.load(Ordering::SeqCst) > 0);
        assert!(token.root.control.settled.get());
        assert!(token.root.owner.upgrade().is_none());
        assert!(!native_cleanup_no_yield());
        let scope = CLOSED.with(|slot| slot.borrow_mut().take().unwrap());
        assert_eq!(scope.local.generation.active.load(Ordering::SeqCst), 0);
        let (next_scope, next_store) = {
            let mut write = store.store().write();
            scope.successor(&store, &mut write.as_store_mut()).unwrap()
        };
        assert_eq!(
            futures::executor::block_on(
                instance
                    .exports
                    .get_function("fresh")
                    .unwrap()
                    .call_async(&next_store, vec![])
            )
            .unwrap()[0]
                .i32(),
            Some(42)
        );
        drop(next_scope);
        TOKEN.with(|slot| drop(slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        assert!(CoroutineContext::current_root().is_none());
    }

    #[derive(Clone, Copy)]
    enum CleanupProof {
        Parked,
        Retiring,
        Queued,
        Foreign,
        Driver,
    }

    // Match native_frame_proof: real borrowed-root stacks and TLS-held weak
    // metadata model external scopes without introducing native pointers.
    fn native_cleanup_blocker_proof(mode: CleanupProof) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        type RootCall = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;
        thread_local! {
            static TOKEN: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static CALL: RefCell<Option<RootCall>> = const { RefCell::new(None) };
        }
        struct Probe {
            drops: Arc<AtomicUsize>,
            exited: Arc<AtomicBool>,
            cleanup_in_driver: bool,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(
                    self.exited.load(Ordering::SeqCst),
                    "cleanup frame was still entered"
                );
                if self.cleanup_in_driver {
                    let current = WeakNativeInvocation::current().unwrap();
                    assert!(current.is_current());
                    assert!(current.root.owner.upgrade().is_none());
                    let cleanup = unsafe { NativeCleanupGuard::enter(Some(&current)) }.unwrap();
                    assert!(cleanup.root.is_none() && cleanup.gate.is_none());
                    assert!(native_cleanup_no_yield());
                    // This inner modeled cleanup also exits before SDK Drop.
                    drop(cleanup);
                    assert!(!native_cleanup_no_yield());
                }
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            drops: Arc<AtomicUsize>,
            exited: Arc<AtomicBool>,
            cleanup_in_driver: bool,
        }
        let mut store = Store::default();
        let ready = Arc::new(AtomicBool::new(false));
        let drops = Arc::new(AtomicUsize::new(0));
        let exited = Arc::new(AtomicBool::new(false));
        let parent = crate::FunctionEnv::new(
            &mut store,
            Parent {
                leaf: None,
                drops: drops.clone(),
                exited: exited.clone(),
                cleanup_in_driver: matches!(mode, CleanupProof::Driver),
            },
        );
        let body_ready = ready.clone();
        let leaf = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            move |_| {
                let ready = body_ready.clone();
                async move {
                    std::future::poll_fn(|_| {
                        if ready.load(Ordering::SeqCst) {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    Ok(vec![Value::I32(42)])
                }
            },
        );
        let bridge = Function::new_typed_with_env(
            &mut store,
            &parent,
            |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                TOKEN.with(|slot| {
                    assert!(
                        slot.borrow_mut()
                            .replace(WeakNativeInvocation::current().unwrap())
                            .is_none()
                    )
                });
                let (data, mut derived) = env.data_and_store_mut();
                let _probe = Probe {
                    drops: data.drops.clone(),
                    exited: data.exited.clone(),
                    cleanup_in_driver: data.cleanup_in_driver,
                };
                data.leaf.as_ref().unwrap().call(&mut derived)
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "leaf" (func $leaf (result i32)))
            (import "proof" "bridge" (func $bridge (result i32)))
            (func (export "run") (result i32) call $bridge)
            (func (export "leaf") (result i32) call $leaf)
            (func (export "fresh") (result i32) i32.const 42))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "leaf" => leaf, "bridge" => bridge }},
        )
        .unwrap();
        parent.as_mut(&mut store).leaf =
            Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
        let store = store.into_async();
        let mut call: Option<RootCall> = Some(Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        ));
        let noop = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        let token = TOKEN.with(|slot| slot.borrow_mut().take().unwrap());
        if matches!(mode, CleanupProof::Foreign) {
            TOKEN.with(|slot| *slot.borrow_mut() = Some(token.clone()));
            CALL.with(|slot| *slot.borrow_mut() = call.take());
            let mut other = Store::default();
            let stack_drops = drops.clone();
            let header_exited = exited.clone();
            let foreign = Function::new_typed(&mut other, move || -> i32 {
                let current = WeakNativeInvocation::current().unwrap();
                let ancestor = TOKEN.with(|slot| slot.borrow_mut().take().unwrap());
                let cleanup = unsafe { NativeCleanupGuard::enter(Some(&ancestor)) }.unwrap();
                assert!(native_cleanup_no_yield());
                drop(CALL.with(|slot| slot.borrow_mut().take().unwrap()));
                assert!(ancestor.retirement_error().is_some());
                assert!(current.retirement_error().is_none());
                assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
                header_exited.store(true, Ordering::SeqCst);
                drop(cleanup);
                assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
                assert!(current.is_current());
                assert!(current.retirement_error().is_none());
                assert!(!native_cleanup_no_yield());
                42
            });
            assert_eq!(
                futures::executor::block_on(foreign.call_async(&other.into_async(), vec![]))
                    .unwrap()[0]
                    .i32(),
                Some(42)
            );
        } else {
            let ordinary = matches!(mode, CleanupProof::Retiring | CleanupProof::Queued)
                .then(|| unsafe { token.enter_descendant_frame() }.unwrap());
            if matches!(mode, CleanupProof::Retiring) {
                drop(call.take());
                assert!(token.retirement_error().is_some());
            }
            if matches!(mode, CleanupProof::Queued) {
                ready.store(true, Ordering::SeqCst);
                assert!(
                    call.as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut Context::from_waker(&noop))
                        .is_pending()
                );
                assert!(
                    token
                        .root
                        .owner
                        .upgrade()
                        .unwrap()
                        .frame_resume_waiting
                        .get()
                );
            }
            let cleanup = unsafe { NativeCleanupGuard::enter(Some(&token)) }.unwrap();
            let parked_custody = cleanup.root.is_some() && cleanup.gate.is_some();
            drop(ordinary);
            if matches!(mode, CleanupProof::Retiring | CleanupProof::Driver) {
                drop(call.take());
            } else {
                ready.store(true, Ordering::SeqCst);
                assert!(
                    call.as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut Context::from_waker(&noop))
                        .is_pending()
                );
            }
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            exited.store(true, Ordering::SeqCst);
            drop(cleanup);
            assert!(!native_cleanup_no_yield());
            if let Some(mut call) = call.take() {
                let Poll::Ready(Ok(values)) = call.as_mut().poll(&mut Context::from_waker(&noop))
                else {
                    panic!("ancestor did not resume after synchronous cleanup exit");
                };
                assert_eq!(values[0].i32(), Some(42));
                drop(call);
            }
            assert!(
                parked_custody,
                "host cleanup did not capture its parked root and gate"
            );
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(token.root.owner.upgrade().is_none());
        let Err(error) = (unsafe { NativeCleanupGuard::enter(Some(&token)) }) else {
            panic!("stale cleanup token was accepted without a physical caller");
        };
        assert!(error.message().contains("already ended"));
        assert_eq!(
            futures::executor::block_on(
                instance
                    .exports
                    .get_function("fresh")
                    .unwrap()
                    .call_async(&store, vec![])
            )
            .unwrap()[0]
                .i32(),
            Some(42)
        );
        assert!(!native_cleanup_no_yield());
        assert!(StoreContext::is_empty());
        assert!(CoroutineContext::current_root().is_none());
    }

    #[test]
    fn host_cleanup_blocks_resume_and_retirement_without_adding_native_ancestry() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::host_cleanup_blocks_resume_and_retirement_without_adding_native_ancestry",
        ) {
            return;
        }
        for mode in [
            CleanupProof::Parked,
            CleanupProof::Retiring,
            CleanupProof::Queued,
            CleanupProof::Foreign,
            CleanupProof::Driver,
        ] {
            native_cleanup_blocker_proof(mode);
        }
    }

    #[test]
    fn host_cleanup_rejects_new_async_roots_and_pending_host_futures() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::host_cleanup_rejects_new_async_roots_and_pending_host_futures",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct PendingProbe {
            ready: Arc<AtomicBool>,
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        }
        impl Future for PendingProbe {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                self.polls.fetch_add(1, Ordering::SeqCst);
                if self.ready.load(Ordering::SeqCst) {
                    Poll::Ready(Ok(vec![Value::I32(42)]))
                } else {
                    Poll::Pending
                }
            }
        }
        impl Drop for PendingProbe {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        let mut store = Store::default();
        let entries = Arc::new(AtomicUsize::new(0));
        let entered = entries.clone();
        let fresh = Function::new_typed(&mut store, move || -> i32 {
            entered.fetch_add(1, Ordering::SeqCst);
            42
        });
        let polls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let polled = polls.clone();
        let dropped = drops.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let refused = Arc::new(AtomicBool::new(false));
        let leaf_ready = ready.clone();
        struct Synchronous {
            leaf: Option<crate::TypedFunction<(), i32>>,
            immediate: Option<crate::TypedFunction<(), i32>>,
            refused: Arc<AtomicBool>,
        }
        let state = crate::FunctionEnv::new(
            &mut store,
            Synchronous {
                leaf: None,
                immediate: None,
                refused: refused.clone(),
            },
        );
        let pending_leaf = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            move |_| PendingProbe {
                ready: leaf_ready.clone(),
                polls: polled.clone(),
                drops: dropped.clone(),
            },
        );
        let immediate = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            |_| async { Ok(vec![Value::I32(42)]) },
        );
        let synchronous = Function::new_typed_with_env(
            &mut store,
            &state,
            |mut env: crate::FunctionEnvMut<Synchronous>| -> Result<i32, RuntimeError> {
                let current = WeakNativeInvocation::current().unwrap();
                let cleanup = unsafe { NativeCleanupGuard::enter(Some(&current)) }.unwrap();
                assert!(cleanup.root.is_none() && cleanup.gate.is_none());
                assert!(native_cleanup_no_yield());
                let (data, mut derived) = env.data_and_store_mut();
                // Ready and Pending both use the supported borrowed Wasm
                // suffix. The runtime may yield even an immediate future when
                // NoYield is deliberately omitted by the sensitivity probe.
                assert_eq!(
                    data.immediate.as_ref().unwrap().call(&mut derived).unwrap(),
                    42
                );
                match data.leaf.as_ref().unwrap().call(&mut derived) {
                    Err(error) if error.is_host_abort() => return Err(error),
                    Err(error) => data.refused.store(
                        error.message().contains("synchronous native cleanup"),
                        Ordering::SeqCst,
                    ),
                    // A defective modeled yield is completed normally before
                    // the post-cleanup discriminator; no forced abort is needed.
                    Ok(value) => assert_eq!(value, 42),
                }
                assert!(current.is_current());
                // The modeled engine entry has exited before its SDK guard.
                drop(cleanup);
                assert!(!native_cleanup_no_yield());
                Ok(42)
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "pending" (func $pending (result i32)))
            (import "proof" "immediate" (func $immediate (result i32)))
            (import "proof" "bridge" (func $bridge (result i32)))
            (func (export "run") (result i32) call $bridge)
            (func (export "leaf") (result i32) call $pending)
            (func (export "immediate") (result i32) call $immediate))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "pending" => pending_leaf, "immediate" => immediate, "bridge" => synchronous }},
        )
        .unwrap();
        state.as_mut(&mut store).leaf =
            Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
        state.as_mut(&mut store).immediate = Some(
            instance
                .exports
                .get_typed_function(&store, "immediate")
                .unwrap(),
        );
        let store = store.into_async();
        let locked = store.store().write();
        let cleanup = unsafe { NativeCleanupGuard::enter(None) }.unwrap();
        let nested = unsafe { NativeCleanupGuard::enter(None) }.unwrap();
        drop(nested);
        assert!(native_cleanup_no_yield());
        let mut call = Box::pin(fresh.call_async(&store, vec![]));
        let noop = futures::task::noop_waker();
        let rejected = {
            // Wake callbacks clear logical retirement metadata. Physical
            // cleanup NoYield must remain in force independently of that scope.
            let _metadata = DetachedRootMetadata::new();
            matches!(
                call.as_mut().poll(&mut Context::from_waker(&noop)),
                Poll::Ready(Err(ref error)) if error.message().contains("new native async root")
            )
        };
        assert_eq!(entries.load(Ordering::SeqCst), 0);
        drop(call);
        drop(locked);
        // The unmanaged cleanup frame has synchronously completed.
        drop(cleanup);
        assert!(!native_cleanup_no_yield());
        assert!(
            rejected,
            "new async root waited for a Store lock during cleanup"
        );
        assert_eq!(
            futures::executor::block_on(fresh.call_async(&store, vec![])).unwrap()[0].i32(),
            Some(42)
        );
        assert_eq!(entries.load(Ordering::SeqCst), 1);
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let completed = matches!(
            call.as_mut().poll(&mut Context::from_waker(&noop)),
            Poll::Ready(Ok(ref values)) if values[0].i32() == Some(42)
        );
        let resumed = if completed {
            true
        } else {
            ready.store(true, Ordering::SeqCst);
            matches!(
                call.as_mut().poll(&mut Context::from_waker(&noop)),
                Poll::Ready(Ok(ref values)) if values[0].i32() == Some(42)
            )
        };
        drop(call);
        assert!(
            resumed,
            "controlled cleanup fixture did not finish its root"
        );
        assert!(
            completed && refused.load(Ordering::SeqCst),
            "native cleanup allowed a borrowed host future to suspend"
        );
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(!native_cleanup_no_yield());
        assert!(StoreContext::is_empty());
        assert!(CoroutineContext::current_root().is_none());
    }

    #[test]
    fn gate_wake_panics_do_not_skip_last_frame_or_loan_stack_retirement() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::gate_wake_panics_do_not_skip_last_frame_or_loan_stack_retirement",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static TOKEN: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static LOAN: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
        }
        struct StackPanic;
        struct GateWakePanic;
        struct OuterPanic;
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(StackPanic);
            }
        }
        struct WakePanic(AtomicUsize);
        impl std::task::Wake for WakePanic {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                assert!(CoroutineContext::current_root().is_none());
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(GateWakePanic);
            }
        }
        enum Guard {
            Frame(NativeFrameGuard),
            Read(NativeLoanReadGuard),
            Write(NativeLoanWriteGuard),
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            dropped: Arc<AtomicUsize>,
        }
        for case in 0..6 {
            let kind = case % 3;
            let outer_unwind = case >= 3;
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let parent = crate::FunctionEnv::new(
                &mut store,
                Parent {
                    leaf: None,
                    dropped: drops.clone(),
                },
            );
            let pending = Function::new_async(
                &mut store,
                crate::FunctionType::new([], [crate::Type::I32]),
                |_| {
                    let current = CoroutineContext::current_root().unwrap();
                    let loan = current_store_loan(current.control.store_id).unwrap();
                    LOAN.with(|slot| assert!(slot.borrow_mut().replace(loan).is_none()));
                    std::future::pending::<Result<Vec<Value>, RuntimeError>>()
                },
            );
            let bridge = Function::new_typed_with_env(
                &mut store,
                &parent,
                |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                    TOKEN.with(|slot| {
                        assert!(
                            slot.borrow_mut()
                                .replace(WeakNativeInvocation::current().unwrap())
                                .is_none()
                        )
                    });
                    let (data, mut store) = env.data_and_store_mut();
                    let _probe = Probe(data.dropped.clone());
                    data.leaf.as_ref().unwrap().call(&mut store)
                },
            );
            let module = Module::new(
                &store,
                r#"(module
                (import "proof" "pending" (func $pending (result i32)))
                (import "proof" "bridge" (func $bridge (result i32)))
                (func (export "run") (result i32) call $bridge)
                (func (export "leaf") (result i32) call $pending)
                (func (export "fresh") (result i32) i32.const 42))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &crate::imports! { "proof" => { "pending" => pending, "bridge" => bridge }},
            )
            .unwrap();
            parent.as_mut(&mut store).leaf =
                Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
            let store = store.into_async();
            let mut call = Box::pin(
                instance
                    .exports
                    .get_function("run")
                    .unwrap()
                    .call_async(&store, vec![]),
            );
            let noop = futures::task::noop_waker();
            assert!(
                call.as_mut()
                    .poll(&mut Context::from_waker(&noop))
                    .is_pending()
            );
            let token = TOKEN.with(|slot| slot.borrow_mut().take().unwrap());
            let capability = LOAN.with(|slot| slot.borrow_mut().take().unwrap());
            let loan = capability.loan.as_ref().unwrap();
            let (guard, gate) = match kind {
                0 => {
                    let frame = unsafe { token.enter_descendant_frame() }.unwrap();
                    let gate = token
                        .root
                        .owner
                        .upgrade()
                        .unwrap()
                        .frame_gate
                        .get()
                        .unwrap()
                        .clone();
                    (Guard::Frame(frame), gate)
                }
                1 => (Guard::Read(loan.try_read().unwrap()), loan.gate.clone()),
                _ => (Guard::Write(loan.try_write().unwrap()), loan.gate.clone()),
            };
            let wakes = Arc::new(WakePanic(AtomicUsize::new(0)));
            let waker = Waker::from(wakes.clone());
            let mut writer = Box::pin(gate.write());
            assert!(
                writer
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            drop(call);
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            // The modeled outside frame/loan use has ended before guard release.
            // Catch each concrete guard Drop so the enum owns no cleanup phase.
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if outer_unwind {
                    let _guard = guard;
                    std::panic::panic_any(OuterPanic);
                }
                match guard {
                    Guard::Frame(value) => drop(value),
                    Guard::Read(value) => drop(value),
                    Guard::Write(value) => drop(value),
                }
            }))
            .expect_err("the first panic must propagate after complete stack retirement");
            if outer_unwind {
                assert!(
                    panic.is::<OuterPanic>(),
                    "guard cleanup replaced the outer panic"
                );
            } else {
                assert!(
                    panic.is::<GateWakePanic>(),
                    "last stack panic replaced the first gate wake panic"
                );
            }
            assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let Poll::Ready(write) = writer.as_mut().poll(&mut Context::from_waker(&noop)) else {
                panic!("gate was not released after its Waker panicked");
            };
            drop(write);
            drop(writer);
            let fresh = instance.exports.get_function("fresh").unwrap();
            assert_eq!(
                futures::executor::block_on(fresh.call_async(&store, vec![])).unwrap()[0].i32(),
                Some(42)
            );
            assert!(StoreContext::is_empty());
            assert!(CoroutineContext::current_root().is_none());
        }
    }

    #[test]
    fn a_scope_waiter_panic_does_not_replace_a_stack_destructor_panic() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::a_scope_waiter_panic_does_not_replace_a_stack_destructor_panic",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! { static FRAME: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) }; }
        struct StackPanic;
        struct WaiterPanic;
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(StackPanic);
            }
        }
        struct WakePanic(AtomicUsize);
        impl std::task::Wake for WakePanic {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                assert!(CoroutineContext::current_root().is_none());
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(WaiterPanic);
            }
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            dropped: Arc<AtomicUsize>,
        }
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let parent = crate::FunctionEnv::new(
            &mut store,
            Parent {
                leaf: None,
                dropped: drops.clone(),
            },
        );
        let pending = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [crate::Type::I32]),
            |_| std::future::pending::<Result<Vec<Value>, RuntimeError>>(),
        );
        let bridge = Function::new_typed_with_env(
            &mut store,
            &parent,
            |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                FRAME.with(|frame| *frame.borrow_mut() = WeakNativeInvocation::current());
                let (data, mut store) = env.data_and_store_mut();
                let _probe = Probe(data.dropped.clone());
                data.leaf.as_ref().unwrap().call(&mut store)
            },
        );
        let module = Module::new(
            &store,
            r#"(module
          (import "proof" "pending" (func $pending (result i32)))
          (import "proof" "bridge" (func $bridge (result i32)))
          (func (export "run") (result i32) call $bridge)
          (func (export "leaf") (result i32) call $pending)
          (func (export "fresh") (result i32) i32.const 42))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "pending" => pending, "bridge" => bridge }},
        )
        .unwrap();
        parent.as_mut(&mut store).leaf =
            Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
        let mut scope = NativeInvocationScope::bind(&mut store).unwrap();
        let store = store.into_async();
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let noop = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        let token = FRAME.with(|frame| frame.borrow_mut().take().unwrap());
        // The modeled external frame releases its pin before this caller waits
        // for physical settlement; no C++ continuation is involved in the proof.
        let frame = unsafe { token.enter_descendant_frame() }.unwrap();
        scope.close();
        drop(call);
        let wakes = Arc::new(WakePanic(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut wait = Box::pin(scope.close_and_wait());
        assert!(
            wait.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(frame)))
            .expect_err("first user destructor panic must propagate after settlement");
        assert!(panic.is::<StackPanic>());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            wait.as_mut().poll(&mut Context::from_waker(&noop)),
            Poll::Ready(Ok(()))
        ));
        drop(wait);
        assert_eq!(scope.local.generation.active.load(Ordering::SeqCst), 0);
        let (next_scope, next) = {
            let mut guard = store.store().write();
            scope.successor(&store, &mut guard.as_store_mut()).unwrap()
        };
        let fresh = instance.exports.get_function("fresh").unwrap();
        assert_eq!(
            futures::executor::block_on(fresh.call_async(&next, vec![])).unwrap()[0].i32(),
            Some(42)
        );
        drop(next_scope);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_closed_scope_waits_for_the_last_physical_frame_pin() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::a_closed_scope_waits_for_the_last_physical_frame_pin",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static FRAME: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
        }
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let pending = Function::new_typed(&mut store, {
            let dropped = dropped.clone();
            move || -> Result<i32, RuntimeError> {
                FRAME.with(|frame| *frame.borrow_mut() = WeakNativeInvocation::current());
                let _probe = Probe(dropped.clone());
                block_on_host_future(std::future::pending()).map_err(|error| match error {
                    AsyncRuntimeError::RuntimeError(error) => error,
                    _ => RuntimeError::new("scope fixture failed to suspend"),
                })?;
                Ok(42)
            }
        });
        let fresh = Function::new_typed(&mut store, || -> i32 { 42 });
        let mut scope = NativeInvocationScope::bind(&mut store).unwrap();
        let store = store.into_async();
        let weak = store.downgrade();
        let mut call = Box::pin(pending.call_async(&store, vec![]));
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(call.as_mut().poll(&mut cx).is_pending());
        let token = FRAME.with(|frame| frame.borrow_mut().take().unwrap());
        // This modeled external scope has no C++ stack. Its caller explicitly
        // waits for settlement after releasing the scope, as the unsafe API requires.
        let frame = unsafe { token.enter_descendant_frame() }.unwrap();
        scope.close();
        drop(call);
        assert_eq!(scope.local.generation.active.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        struct Waiter(AtomicUsize);
        impl std::task::Wake for Waiter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let retained = Arc::new(Waiter(AtomicUsize::new(0)));
        let owned_waker = Waker::from(retained.clone());
        {
            let mut canceled_wait = Box::pin(scope.close_and_wait());
            assert!(
                canceled_wait
                    .as_mut()
                    .poll(&mut Context::from_waker(&owned_waker))
                    .is_pending()
            );
            assert_eq!(Arc::strong_count(&retained), 3);
        }
        assert_eq!(
            Arc::strong_count(&retained),
            2,
            "canceled observer retained its task Waker"
        );
        assert!(scope.local.generation.waiter.lock().unwrap().is_none());
        let mut wait = Box::pin(scope.close_and_wait());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        drop(frame);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        drop(wait);
        assert_eq!(
            retained.0.load(Ordering::SeqCst),
            0,
            "canceled observer was spuriously woken"
        );
        assert!(weak.upgrade().is_none());
        assert!(
            futures::executor::block_on(fresh.call_async(&store, vec![]))
                .unwrap_err()
                .is_host_abort()
        );
        let (next_scope, next_store) = {
            let mut guard = store.store().write();
            scope.successor(&store, &mut guard.as_store_mut()).unwrap()
        };
        assert_eq!(
            futures::executor::block_on(fresh.call_async(&next_store, vec![])).unwrap()[0].i32(),
            Some(42)
        );
        assert!(futures::executor::block_on(fresh.call_async(&store, vec![])).is_err());
        {
            let mut old_guard = store.store().write();
            assert!(fresh.call(&mut old_guard, &[]).unwrap_err().is_host_abort());
            let typed = fresh.typed::<(), i32>(&old_guard).unwrap();
            assert!(typed.call(&mut old_guard).unwrap_err().is_host_abort());
        }
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _old_guard = store.store().write();
            panic!("write-origin unwind proof");
        }));
        assert!(panic.is_err());
        assert!(
            store
                .inner
                .try_read()
                .unwrap()
                .native_access_origin
                .is_none()
        );
        {
            let mut new_guard = next_store.store().write();
            assert_eq!(fresh.call(&mut new_guard, &[]).unwrap()[0].i32(), Some(42));
        }
        let new_weak = next_store.downgrade();
        drop(next_store);
        // This is the sole remaining strong cap. The failure must come from
        // its superseded generation, not the lock's ordinary strong count.
        let store = match store.into_store() {
            Err(store) => store,
            Ok(_) => panic!("superseded capability consumed the successor Store"),
        };
        let upgraded = new_weak
            .upgrade()
            .expect("stale consume invalidated the new weak capability");
        assert_eq!(
            futures::executor::block_on(fresh.call_async(&upgraded, vec![])).unwrap()[0].i32(),
            Some(42)
        );
        drop(upgraded);
        assert!(futures::executor::block_on(fresh.call_async(&store, vec![])).is_err());
        drop(next_scope);
    }

    #[test]
    fn dropping_a_scope_cleans_its_weak_row_after_a_waker_panics() {
        struct WakePanic;
        struct ScopeDropPanic;
        impl std::task::Wake for WakePanic {
            fn wake(self: Arc<Self>) {
                std::panic::panic_any(ScopeDropPanic);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                std::panic::panic_any(ScopeDropPanic);
            }
        }
        let mut store = Store::default();
        let function = Function::new_typed(&mut store, || -> i32 {
            panic!("unstarted scope entered the engine")
        });
        let scope = NativeInvocationScope::bind(&mut store).unwrap();
        let id = scope.local.generation.id;
        let store = store.into_async();
        let locked = store.store().write();
        let mut call = Box::pin(function.call_async(&store, vec![]));
        let waker = Waker::from(Arc::new(WakePanic));
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(scope)))
            .expect_err("scope close Waker panic must propagate after metadata cleanup");
        assert!(panic.is::<ScopeDropPanic>());
        INVOCATION_SCOPES.with(|scopes| assert!(!scopes.borrow().contains_key(&id)));
        let noop = futures::task::noop_waker();
        assert!(matches!(
            call.as_mut().poll(&mut Context::from_waker(&noop)),
            Poll::Ready(Err(_))
        ));
        // Retirement removes the lock waiter before unlock can wake it again.
        drop(locked);
    }

    #[test]
    fn unstarted_scope_roots_do_not_delay_close_or_enter_the_engine() {
        let mut store = Store::default();
        let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let function = Function::new_typed(&mut store, {
            let entered = entered.clone();
            move || -> i32 {
                entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                42
            }
        });
        let mut scope = NativeInvocationScope::bind(&mut store).unwrap();
        let store = store.into_async();
        let locked = store.store().write();
        let mut blocked = Box::pin(function.call_async(&store, vec![]));
        let mut dormant = Box::pin(function.call_async(&store, vec![]));
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(blocked.as_mut().poll(&mut cx).is_pending());
        let mut wait = Box::pin(scope.close_and_wait());
        assert!(matches!(wait.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        drop(wait);
        assert!(matches!(
            blocked.as_mut().poll(&mut cx),
            Poll::Ready(Err(_))
        ));
        assert!(matches!(
            dormant.as_mut().poll(&mut cx),
            Poll::Ready(Err(_))
        ));
        drop(locked);
        let mut direct = store.store().write();
        assert!(function.call(&mut direct, &[]).unwrap_err().is_host_abort());
        assert_eq!(entered.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn scope_metadata_is_send_sync_and_rejects_a_foreign_thread() {
        // Store itself is already !Send (OnCalledHandler has no Send bound).
        // New Store fields carry only Send + Sync generation metadata, not Rc.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NativeInvocationGeneration>();
        let mut store = Store::default();
        let function = Function::new_typed(&mut store, || -> i32 { 42 });
        let scope = NativeInvocationScope::bind(&mut store).unwrap();
        let generation = scope.local.generation.clone();
        let generation = std::thread::spawn(move || {
            assert!(generation.check().is_err());
            generation
        })
        .join()
        .unwrap();
        assert!(Arc::ptr_eq(&generation, &scope.local.generation));
        generation.check().unwrap();
        assert_eq!(function.call(&mut store, &[]).unwrap()[0].i32(), Some(42));
        drop(scope);
        assert!(function.call(&mut store, &[]).unwrap_err().is_host_abort());
    }

    #[test]
    fn closing_inside_a_callback_restores_the_caller_and_rejects_self_wait() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::closing_inside_a_callback_restores_the_caller_and_rejects_self_wait",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! { static SCOPE: RefCell<Option<NativeInvocationScope>> = const { RefCell::new(None) }; }
        struct WakeProbe {
            wakes: AtomicUsize,
            logical_body: bool,
        }
        impl std::task::Wake for WakeProbe {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                assert!(CoroutineContext::current_root().is_none());
                // A physical callback retains a hidden Store prefix. A host
                // body runs outside that prefix; neither wake has root authority.
                assert_eq!(StoreContext::async_root_admissible(), self.logical_body);
                self.wakes.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn close_here() {
            let before = CoroutineContext::get_current();
            SCOPE.with(|slot| {
                let mut scope = slot.borrow_mut();
                let scope = scope.as_mut().unwrap();
                let mut wait = Box::pin(scope.close_and_wait());
                let waker = futures::task::noop_waker();
                assert!(matches!(
                    wait.as_mut().poll(&mut Context::from_waker(&waker)),
                    Poll::Ready(Err(_))
                ));
            });
            assert_eq!(CoroutineContext::get_current(), before);
        }
        for logical_body in [false, true] {
            let wakes = Arc::new(WakeProbe {
                wakes: AtomicUsize::new(0),
                logical_body,
            });
            let waker = Waker::from(wakes.clone());
            let mut store = Store::default();
            let close = if logical_body {
                Function::new_async(
                    &mut store,
                    crate::FunctionType::new([], [crate::Type::I32]),
                    |_| async {
                        close_here();
                        Ok(vec![Value::I32(42)])
                    },
                )
            } else {
                Function::new_typed(&mut store, || -> i32 {
                    close_here();
                    42
                })
            };
            let scope = NativeInvocationScope::bind(&mut store).unwrap();
            SCOPE.with(|slot| *slot.borrow_mut() = Some(scope));
            let store = store.into_async();
            let mut call = Box::pin(close.call_async(&store, vec![]));
            let Poll::Ready(Err(error)) = call.as_mut().poll(&mut Context::from_waker(&waker))
            else {
                panic!("closed callback returned caught success");
            };
            assert!(error.is_host_abort());
            assert_eq!(wakes.wakes.load(Ordering::SeqCst), 1);
            drop(call);
            let mut scope = SCOPE.with(|slot| slot.borrow_mut().take().unwrap());
            futures::executor::block_on(scope.close_and_wait()).unwrap();
            assert!(StoreContext::is_empty());
        }
    }

    #[test]
    fn finished_and_panicked_roots_reject_pins_before_the_future_is_dropped() {
        thread_local! {
            static ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
        }
        for panics in [false, true] {
            let mut store = Store::default();
            let function = Function::new_typed(&mut store, move || -> i32 {
                let root = CoroutineContext::current_root().unwrap();
                ROOT.with(|slot| assert!(slot.borrow_mut().replace(root).is_none()));
                assert!(!panics, "root lifecycle proof");
                42
            });
            let mut call = Box::pin(AsyncCallFuture::new(
                function.into_sys(),
                store.into_async(),
                vec![],
            ));
            let waker = futures::task::noop_waker();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }));
            assert_eq!(result.is_err(), panics);
            if let Ok(result) = result {
                assert!(matches!(result, Poll::Ready(Ok(_))));
            }
            let root = ROOT.with(|slot| slot.borrow_mut().take().unwrap());
            assert_eq!(root.owner.strong_count(), 1);
            assert!(root.try_pin().is_none());
            drop(call);
            assert_eq!(root.owner.strong_count(), 0);
            assert!(StoreContext::is_empty());
            CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        }
    }

    #[test]
    fn an_external_pin_retains_the_actual_stack_until_its_last_owner_drops() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::an_external_pin_retains_the_actual_stack_until_its_last_owner_drops",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
            static PIN: RefCell<Option<NativeRootPin>> = const { RefCell::new(None) };
        }
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct PendingBody(Probe);
        impl Future for PendingBody {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                let _ = &self.0;
                Poll::Pending
            }
        }
        let stack_drops = Arc::new(AtomicUsize::new(0));
        let body_drops = Arc::new(AtomicUsize::new(0));
        let continued = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let bridge = Function::new_typed(&mut store, {
            let stack_drops = stack_drops.clone();
            let body_drops = body_drops.clone();
            let continued = continued.clone();
            move || -> Result<i32, RuntimeError> {
                let root = CoroutineContext::current_root().expect("root captured before entry");
                assert!(root.try_pin().is_none(), "a root cannot own its own stack");
                ROOT.with(|slot| assert!(slot.borrow_mut().replace(root).is_none()));
                let _stack = Probe(stack_drops.clone());
                block_on_host_future(PendingBody(Probe(body_drops.clone()))).map_err(|error| {
                    match error {
                        AsyncRuntimeError::RuntimeError(error) => error,
                        AsyncRuntimeError::YieldOutsideAsyncContext => {
                            RuntimeError::new("stack ownership proof did not enter an async root")
                        }
                    }
                })?;
                continued.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            }
        });
        let fresh = Function::new_typed(&mut store, || -> i32 { 9 });
        let store = store.into_async();
        let mut call = Box::pin(AsyncCallFuture::new(
            bridge.into_sys(),
            store.store(),
            vec![],
        ));
        let waker = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let root = ROOT.with(|slot| slot.borrow_mut().take().unwrap());
        assert_eq!(root.owner.strong_count(), 1, "the stack holds only Weak");
        let pin = root.try_pin().expect("outside operation retains root");
        drop(call);
        assert_eq!(body_drops.load(Ordering::SeqCst), 1);
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        assert!(root.try_pin().is_none(), "retirement rejects a new pin");
        PIN.with(|slot| assert!(slot.borrow_mut().replace(pin).is_none()));

        // Release the last pin from a real foreign root. Its current context
        // and VM trap/interrupt state must survive the deferred final unwind.
        let mut foreign_store = Store::default();
        let release = Function::new_typed(&mut foreign_store, || -> i32 {
            let current = CoroutineContext::get_current().expect("foreign root active");
            drop(PIN.with(|slot| slot.borrow_mut().take().unwrap()));
            assert_eq!(CoroutineContext::get_current(), Some(current));
            42
        });
        let result =
            futures::executor::block_on(release.call_async(&foreign_store.into_async(), vec![]))
                .unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
        assert_eq!(body_drops.load(Ordering::SeqCst), 1);
        assert_eq!(continued.load(Ordering::SeqCst), 0);
        assert_eq!(root.owner.strong_count(), 0);
        assert!(root.try_pin().is_none());
        let result = futures::executor::block_on(fresh.call_async(&store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(9));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[test]
    fn a_panicking_retirement_yield_keeps_the_borrowed_stack_owned() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::a_panicking_retirement_yield_keeps_the_borrowed_stack_owned",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static BASE: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
        }
        struct StackProbe(Arc<AtomicUsize>);
        impl Drop for StackProbe {
            fn drop(&mut self) {
                BASE.with(|slot| {
                    assert!(
                        slot.borrow().as_ref().unwrap().inner.try_write().is_none(),
                        "original write guard unlocked before borrowed stack destruction"
                    )
                });
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct YieldPanic(u32);
        struct PanickingFuture {
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
            stack: Arc<AtomicUsize>,
        }
        impl Future for PanickingFuture {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                self.polls.fetch_add(1, Ordering::SeqCst);
                Poll::Pending
            }
        }
        impl Drop for PanickingFuture {
            fn drop(&mut self) {
                assert_eq!(
                    self.stack.load(Ordering::SeqCst),
                    1,
                    "retirement payload dropped before the borrowed stack was dead"
                );
                BASE.with(|slot| {
                    assert!(
                        slot.borrow().as_ref().unwrap().inner.try_write().is_none(),
                        "retirement payload lost the original write guard"
                    )
                });
                self.drops.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(YieldPanic(73));
            }
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            stack: Arc<AtomicUsize>,
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        }
        let stack_drops = Arc::new(AtomicUsize::new(0));
        let future_polls = Arc::new(AtomicUsize::new(0));
        let future_drops = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let parent = crate::FunctionEnv::new(
            &mut store,
            Parent {
                leaf: None,
                stack: stack_drops.clone(),
                polls: future_polls.clone(),
                drops: future_drops.clone(),
            },
        );
        let wait = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [wasmer_types::Type::I32]),
            |_| std::future::pending::<Result<Vec<Value>, RuntimeError>>(),
        );
        let bridge = Function::new_typed_with_env(
            &mut store,
            &parent,
            |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                let (data, mut store) = env.data_and_store_mut();
                let _stack = StackProbe(data.stack.clone());
                let error = data.leaf.as_ref().unwrap().call(&mut store).unwrap_err();
                assert!(error.is_host_abort());
                let current = CoroutineContext::get_current().expect("borrowed root resumed");
                // Managed import entry refuses a new yield while retiring. Exercise
                // the driver's defensive owned-yield path using this exact root's
                // private yielder, with a valid future whose destructor panics.
                let context = unsafe { &*current };
                context.leave();
                let resumed =
                    unsafe { &*context.yielder }.suspend(AsyncYield(Box::pin(PanickingFuture {
                        polls: data.polls.clone(),
                        drops: data.drops.clone(),
                        stack: data.stack.clone(),
                    })));
                context.enter();
                assert!(
                    matches!(resumed, AsyncResume::HostFutureReady(Err(ref next)) if next.is_host_abort())
                );
                // Even a trusted callback catching cancellation cannot continue Wasm.
                Ok(42)
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "wait" (func $wait (result i32)))
            (import "proof" "bridge" (func $bridge (result i32)))
            (func (export "run") (result i32) call $bridge)
            (func (export "leaf") (result i32) call $wait)
            (func (export "fresh") (result i32) i32.const 9))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "wait" => wait, "bridge" => bridge } },
        )
        .unwrap();
        parent.as_mut(&mut store).leaf =
            Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
        let store = store.into_async();
        BASE.with(|slot| assert!(slot.borrow_mut().replace(store.store()).is_none()));
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let waker = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(call)))
            .expect_err("owned retirement payload panic must propagate after settlement");
        assert_eq!(panic.downcast_ref::<YieldPanic>().unwrap().0, 73);
        assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
        assert_eq!(future_polls.load(Ordering::SeqCst), 0);
        assert_eq!(future_drops.load(Ordering::SeqCst), 1);
        assert!(store.inner.try_write().is_some());
        let fresh = futures::executor::block_on(
            instance
                .exports
                .get_function("fresh")
                .unwrap()
                .call_async(&store, vec![]),
        )
        .unwrap();
        assert_eq!(fresh[0].i32(), Some(9));
        drop(BASE.with(|slot| slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[derive(Clone, Copy)]
    enum FrameProof {
        Resume,
        Cancel,
        CancelInside,
        CancelOnClone,
        CancelOnGateClone,
        Unmanaged,
    }

    #[test]
    fn a_native_descendant_blocks_cross_store_ancestor_resume() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::a_native_descendant_blocks_cross_store_ancestor_resume",
            FrameProof::Resume,
        );
    }

    #[test]
    fn native_cancellation_wakes_a_cross_store_frame_and_stays_sticky_after_exit() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::native_cancellation_wakes_a_cross_store_frame_and_stays_sticky_after_exit",
            FrameProof::Cancel,
        );
    }

    #[test]
    fn native_frame_exit_latches_an_abort_before_a_nonretiring_child_returns_success() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::native_frame_exit_latches_an_abort_before_a_nonretiring_child_returns_success",
            FrameProof::CancelInside,
        );
    }

    #[test]
    fn a_changed_waker_cancels_native_lineage_before_the_hostbody_is_polled() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::a_changed_waker_cancels_native_lineage_before_the_hostbody_is_polled",
            FrameProof::CancelOnClone,
        );
    }

    #[test]
    fn a_ready_resume_gate_rechecks_native_lineage_before_taking_the_original_root() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::a_ready_resume_gate_rechecks_native_lineage_before_taking_the_original_root",
            FrameProof::CancelOnGateClone,
        );
    }

    #[test]
    fn an_unmanaged_native_frame_transports_and_passes_on_ancestor_cancellation() {
        native_frame_proof(
            "backend::sys::async_runtime::tests::an_unmanaged_native_frame_transports_and_passes_on_ancestor_cancellation",
            FrameProof::Unmanaged,
        );
    }

    fn native_frame_proof(name: &str, proof: FrameProof) {
        if !runtime_subprocess(name) {
            return;
        }
        use std::sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        thread_local! {
            static ANCESTOR: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static CHILD: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
            static PARENT_LOAN: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
            static PARENT_CALL: RefCell<Option<RootCall>> = const { RefCell::new(None) };
            static CHILD_LOAN: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
            static CHILD_WRITE: RefCell<Option<crate::StoreAsyncWriteLock>> = const { RefCell::new(None) };
        }
        struct Data {
            leaf: Option<crate::TypedFunction<(), i32>>,
            descendant: bool,
            cancel_parent: bool,
            stack_drops: Arc<AtomicUsize>,
            frame_exited: Arc<AtomicBool>,
            caught: Arc<AtomicUsize>,
            expected: Arc<Mutex<Option<RuntimeError>>>,
        }
        struct StackProbe {
            drops: Arc<AtomicUsize>,
            must_have_exited: Option<Arc<AtomicBool>>,
        }
        impl Drop for StackProbe {
            fn drop(&mut self) {
                if let Some(exited) = &self.must_have_exited {
                    assert!(
                        exited.load(Ordering::SeqCst),
                        "ancestor retired before descendant scope exit"
                    );
                }
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct ExternalScope(Arc<AtomicBool>);
        impl Drop for ExternalScope {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        struct BodyProbe(Arc<AtomicUsize>);
        impl Drop for BodyProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        #[derive(Debug)]
        struct ReturnedError {
            drops: Arc<AtomicUsize>,
            active_context: Arc<AtomicUsize>,
        }
        impl std::fmt::Display for ReturnedError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("host completion error")
            }
        }
        impl std::error::Error for ReturnedError {}
        impl Drop for ReturnedError {
            fn drop(&mut self) {
                if CoroutineContext::get_current().is_some() {
                    self.active_context.fetch_add(1, Ordering::SeqCst);
                }
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct WakeCount(AtomicUsize);
        impl std::task::Wake for WakeCount {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        type RootCall = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;
        struct CancelOnClone {
            clones: AtomicUsize,
            trigger: usize,
            expected: Arc<Mutex<Option<RuntimeError>>>,
        }
        impl CancelOnClone {
            fn waker(self: &Arc<Self>) -> Waker {
                static VTABLE: RawWakerVTable =
                    RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);
                unsafe fn clone(data: *const ()) -> RawWaker {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<CancelOnClone>())
                    });
                    if state.clones.fetch_add(1, Ordering::SeqCst) + 1 == state.trigger {
                        // A valid clone callback can make a contended lock ready
                        // and cancel a foreign ancestor before poll returns.
                        drop(CHILD_WRITE.with(|slot| slot.borrow_mut().take()));
                        let child =
                            CHILD.with(|slot| slot.borrow().as_ref().unwrap().root.control.clone());
                        assert!(!child.retiring.get() && child.reason.get().is_none());
                        drop(PARENT_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
                        let error = ANCESTOR.with(|slot| {
                            slot.borrow().as_ref().unwrap().retirement_error().unwrap()
                        });
                        *state.expected.lock().unwrap() = Some(error);
                        assert!(!child.retiring.get() && child.reason.get().is_none());
                    }
                    RawWaker::new(Arc::into_raw(Arc::clone(&state)).cast(), &VTABLE)
                }
                unsafe fn wake(data: *const ()) {
                    drop(unsafe { Arc::from_raw(data.cast::<CancelOnClone>()) });
                }
                unsafe fn wake_by_ref(_: *const ()) {}
                unsafe fn drop_waker(data: *const ()) {
                    drop(unsafe { Arc::from_raw(data.cast::<CancelOnClone>()) });
                }
                // Every raw value owns one Arc; clone/by_ref borrow without
                // consuming it, and wake/drop reclaim exactly their own Arc.
                unsafe {
                    Waker::from_raw(RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE))
                }
            }
        }
        struct Root {
            store: StoreAsync,
            instance: Instance,
            call: Option<RootCall>,
            ready: Arc<AtomicBool>,
            stack_drops: Arc<AtomicUsize>,
            body_drops: Arc<AtomicUsize>,
            body_polls: Arc<AtomicUsize>,
            error_drops: Arc<AtomicUsize>,
            error_context: Arc<AtomicUsize>,
            caught: Arc<AtomicUsize>,
        }
        fn build_root(
            descendant: bool,
            cancel_parent: bool,
            dynamic_leaf: bool,
            frame_exited: Arc<AtomicBool>,
            expected: Arc<Mutex<Option<RuntimeError>>>,
        ) -> Root {
            let mut store = Store::default();
            let ready = Arc::new(AtomicBool::new(false));
            let body_drops = Arc::new(AtomicUsize::new(0));
            let body_polls = Arc::new(AtomicUsize::new(0));
            let error_drops = Arc::new(AtomicUsize::new(0));
            let error_context = Arc::new(AtomicUsize::new(0));
            let stack_drops = Arc::new(AtomicUsize::new(0));
            let caught = Arc::new(AtomicUsize::new(0));
            let env = crate::FunctionEnv::new(
                &mut store,
                Data {
                    leaf: None,
                    descendant,
                    cancel_parent,
                    stack_drops: stack_drops.clone(),
                    frame_exited,
                    caught: caught.clone(),
                    expected,
                },
            );
            let leaf_ready = ready.clone();
            let body_drop = body_drops.clone();
            let body_poll = body_polls.clone();
            let factory = move || {
                let root = CoroutineContext::current_root().unwrap();
                let loan = current_store_loan(root.control.store_id).unwrap();
                if descendant {
                    CHILD_LOAN.with(|slot| assert!(slot.borrow_mut().replace(loan).is_none()));
                } else {
                    PARENT_LOAN.with(|slot| assert!(slot.borrow_mut().replace(loan).is_none()));
                }
                let ready = leaf_ready.clone();
                let polls = body_poll.clone();
                let probe = BodyProbe(body_drop.clone());
                async move {
                    let _probe = probe;
                    std::future::poll_fn(|_| {
                        polls.fetch_add(1, Ordering::SeqCst);
                        if ready.load(Ordering::SeqCst) {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    Ok::<i32, RuntimeError>(42)
                }
            };
            let leaf = if dynamic_leaf {
                let error_drops = error_drops.clone();
                let error_context = error_context.clone();
                Function::new_async(
                    &mut store,
                    crate::FunctionType::new([], [crate::Type::I32]),
                    move |_| {
                        let future = factory();
                        let drops = error_drops.clone();
                        let active_context = error_context.clone();
                        async move {
                            future.await?;
                            Err::<Vec<Value>, RuntimeError>(RuntimeError::user(Box::new(
                                ReturnedError {
                                    drops,
                                    active_context,
                                },
                            )))
                        }
                    },
                )
            } else {
                Function::new_typed_async(&mut store, factory)
            };
            let bridge = Function::new_typed_with_env(
                &mut store,
                &env,
                |mut env: crate::FunctionEnvMut<Data>| -> Result<i32, RuntimeError> {
                    let current = WeakNativeInvocation::current().unwrap();
                    assert!(current.is_current());
                    let descendant = env.data().descendant;
                    if !descendant {
                        ANCESTOR.with(|slot| {
                            assert!(slot.borrow_mut().replace(current.clone()).is_none())
                        });
                        // The same physical invocation must not acquire a pin or gate.
                        let same = unsafe { current.enter_descendant_frame() }.unwrap();
                        assert!(same._root.is_none() && same._gate.is_none());
                        assert!(
                            current
                                .root
                                .owner
                                .upgrade()
                                .unwrap()
                                .frame_gate
                                .get()
                                .is_none()
                        );
                    } else {
                        CHILD.with(|slot| {
                            assert!(slot.borrow_mut().replace(current.clone()).is_none())
                        });
                    }
                    let (data, mut store) = env.data_and_store_mut();
                    let _stack = StackProbe {
                        drops: data.stack_drops.clone(),
                        must_have_exited: (!descendant).then(|| data.frame_exited.clone()),
                    };
                    let frame = descendant.then(|| {
                        let ancestor =
                            ANCESTOR.with(|slot| slot.borrow().as_ref().unwrap().clone());
                        unsafe { ancestor.enter_descendant_frame() }.unwrap()
                    });
                    // Models an external engine's entry scope. This scope must exit
                    // before frame drops its blocker and possibly the last parent pin.
                    let external_scope =
                        descendant.then(|| ExternalScope(data.frame_exited.clone()));
                    let result = data.leaf.as_ref().unwrap().call(&mut store);
                    if data.cancel_parent {
                        assert_eq!(result.as_ref().unwrap(), &42);
                        let child =
                            CHILD.with(|slot| slot.borrow().as_ref().unwrap().root.control.clone());
                        assert!(!child.retiring.get());
                        assert!(child.reason.get().is_none());
                        assert!(child.native_reason.get().is_none());
                        drop(PARENT_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
                        let error = ANCESTOR.with(|slot| {
                            slot.borrow().as_ref().unwrap().retirement_error().unwrap()
                        });
                        *data.expected.lock().unwrap() = Some(error);
                        // Only the active native edge knows this abort. The
                        // ordinary child retirement path has not seeded it.
                        assert!(!child.retiring.get());
                        assert!(child.reason.get().is_none());
                        assert!(child.native_reason.get().is_none());
                    }
                    drop(external_scope);
                    drop(frame);
                    // A last ancestor pin can synchronously drive its retired
                    // stack here, then must restore this exact physical root.
                    assert!(current.is_current());
                    match result {
                        Ok(value) => Ok(value),
                        Err(error) => {
                            assert!(error.is_host_abort());
                            let expected = data.expected.lock().unwrap();
                            assert!(std::ptr::eq(
                                error.downcast_ref::<std::io::Error>().unwrap(),
                                expected
                                    .as_ref()
                                    .unwrap()
                                    .downcast_ref::<std::io::Error>()
                                    .unwrap()
                            ));
                            data.caught.fetch_add(1, Ordering::SeqCst);
                            // The native edge has already gone, so its sticky reason
                            // must still veto this caught success at import return.
                            Ok(42)
                        }
                    }
                },
            );
            let module = Module::new(
                &store,
                r#"(module
                (import "proof" "bridge" (func $bridge (result i32)))
                (import "proof" "leaf" (func $leaf (result i32)))
                (global $continued (mut i32) (i32.const 0))
                (func (export "run") (result i32)
                    (block $caught (try_table (catch_all $caught)
                        call $bridge i32.const 1 global.set $continued return))
                    i32.const 2 global.set $continued i32.const 0)
                (func (export "leaf") (result i32) call $leaf)
                (func (export "continued") (result i32) global.get $continued)
                (func (export "fresh") (result i32) i32.const 9))"#,
            )
            .unwrap();
            let instance = Instance::new(
                &mut store,
                &module,
                &crate::imports! { "proof" => { "bridge" => bridge, "leaf" => leaf } },
            )
            .unwrap();
            env.as_mut(&mut store).leaf =
                Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
            let store = store.into_async();
            let call = Box::pin(
                instance
                    .exports
                    .get_function("run")
                    .unwrap()
                    .call_async(&store, vec![]),
            );
            Root {
                store,
                instance,
                call: Some(call),
                ready,
                stack_drops,
                body_drops,
                body_polls,
                error_drops,
                error_context,
                caught,
            }
        }
        let exited = Arc::new(AtomicBool::new(false));
        let expected = Arc::new(Mutex::new(None));
        let mut parent = build_root(false, false, false, exited.clone(), expected.clone());
        let noop = futures::task::noop_waker();
        assert!(
            parent
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        let ancestor = ANCESTOR.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert!(!ancestor.is_current());
        let unmanaged = if matches!(proof, FrameProof::Unmanaged) {
            assert!(WeakNativeInvocation::current().is_none());
            let guard = unsafe { ancestor.enter_descendant_frame() }.unwrap();
            assert!(guard.binding.is_none() && guard._root.is_some());
            ANCESTOR.with(|slot| *slot.borrow_mut() = Some(guard.ancestor()));
            Some(guard)
        } else {
            None
        };
        let loan_guard = if matches!(proof, FrameProof::Resume) {
            // A foreign provider frame carries no access to A's Store. Its
            // resume blocker must remain independent of this actual write loan.
            Some(PARENT_LOAN.with(|slot| slot.borrow().as_ref().unwrap().store().write()))
        } else {
            None
        };
        let mut child = build_root(
            true,
            matches!(proof, FrameProof::CancelInside),
            matches!(proof, FrameProof::CancelOnGateClone),
            exited.clone(),
            expected.clone(),
        );
        let child_wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let child_waker = Waker::from(child_wakes.clone());
        assert!(
            child
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&child_waker))
                .is_pending()
        );
        drop(loan_guard);
        assert_ne!(parent.store.id, child.store.id);
        let child_token = CHILD.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert_eq!(child_token.root.control.native_edges.borrow().len(), 1);
        assert!(
            NativeFrameBinding::enter(ancestor.root.control.clone(), &child_token.root.control)
                .is_err()
        );
        assert!(ancestor.root.control.native_edges.borrow().is_empty());

        parent.ready.store(true, Ordering::SeqCst);
        assert!(
            parent
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&noop))
                .is_pending()
        );
        assert_eq!(parent.stack_drops.load(Ordering::SeqCst), 0);
        // A queued resume writer must not be starved by another scope's reader.
        let Err(rejected) = (unsafe { ancestor.enter_descendant_frame() }) else {
            panic!("queued ancestor admitted another native frame");
        };
        assert!(rejected.message().contains("waiting to resume"));
        if matches!(proof, FrameProof::Resume) {
            child.ready.store(true, Ordering::SeqCst);
            assert_eq!(
                futures::executor::block_on(child.call.as_mut().unwrap().as_mut()).unwrap()[0]
                    .i32(),
                Some(42)
            );
            assert!(exited.load(Ordering::SeqCst));
            assert!(child_token.root.control.settled.get());
            assert!(
                child_token
                    .root
                    .owner
                    .upgrade()
                    .unwrap()
                    .stack
                    .borrow()
                    .coroutine
                    .is_none()
            );
            assert!(child_token.root.control.native_edges.borrow().is_empty());
            assert!(child_token.retirement_error().is_none());
            assert_eq!(
                futures::executor::block_on(parent.call.as_mut().unwrap().as_mut()).unwrap()[0]
                    .i32(),
                Some(42)
            );
            drop(parent.call.take());
        } else if matches!(proof, FrameProof::CancelInside) {
            PARENT_CALL.with(|slot| {
                assert!(
                    slot.borrow_mut()
                        .replace(parent.call.take().unwrap())
                        .is_none()
                )
            });
            child.ready.store(true, Ordering::SeqCst);
            let result = child
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&child_waker));
            let Poll::Ready(Err(error)) = result else {
                panic!("native frame exit lost the abort and allowed caught success");
            };
            let original = ancestor.retirement_error().unwrap();
            assert!(std::ptr::eq(
                error.downcast_ref::<std::io::Error>().unwrap(),
                original.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert!(!child_token.root.control.retiring.get());
            assert!(child_token.root.control.reason.get().is_none());
            assert!(std::ptr::eq(
                child_token
                    .root
                    .control
                    .native_reason
                    .get()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap(),
                original.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert!(child_token.root.control.native_edges.borrow().is_empty());
            assert!(child_token.root.control.settled.get());
            assert_eq!(child_token.root.owner.strong_count(), 0);
            assert_eq!(child.caught.load(Ordering::SeqCst), 0);
            assert_eq!(parent.caught.load(Ordering::SeqCst), 1);
            assert_eq!(child_wakes.0.load(Ordering::SeqCst), 1);
            assert!(PARENT_CALL.with(|slot| slot.borrow().is_none()));
        } else if matches!(
            proof,
            FrameProof::CancelOnClone | FrameProof::CancelOnGateClone
        ) {
            let gate_clone = matches!(proof, FrameProof::CancelOnGateClone);
            if gate_clone {
                CHILD_WRITE.with(|slot| {
                    *slot.borrow_mut() = Some(
                        CHILD_LOAN.with(|loan| loan.borrow().as_ref().unwrap().store().write()),
                    );
                });
                child.ready.store(true, Ordering::SeqCst);
            }
            assert_eq!(child.body_polls.load(Ordering::SeqCst), 1);
            PARENT_CALL.with(|slot| {
                assert!(
                    slot.borrow_mut()
                        .replace(parent.call.take().unwrap())
                        .is_none()
                )
            });
            let clone_state = Arc::new(CancelOnClone {
                clones: AtomicUsize::new(0),
                trigger: if gate_clone { 2 } else { 1 },
                expected: expected.clone(),
            });
            let waker = clone_state.waker();
            let result = child
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&waker));
            let Poll::Ready(Err(error)) = result else {
                panic!("Waker callback cancellation was missed");
            };
            let original = ancestor.retirement_error().unwrap();
            assert!(std::ptr::eq(
                error.downcast_ref::<std::io::Error>().unwrap(),
                original.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert_eq!(
                clone_state.clones.load(Ordering::SeqCst),
                clone_state.trigger
            );
            assert_eq!(
                child.body_polls.load(Ordering::SeqCst),
                if gate_clone { 2 } else { 1 },
                "canceled native hostbody was polled again"
            );
            assert_eq!(
                child.caught.load(Ordering::SeqCst),
                1,
                "canceled native stack resumed a successful result"
            );
            assert_eq!(parent.caught.load(Ordering::SeqCst), 1);
            if gate_clone {
                assert_eq!(child.error_drops.load(Ordering::SeqCst), 1);
                assert_eq!(
                    child.error_context.load(Ordering::SeqCst),
                    0,
                    "completed body error dropped on active native stack"
                );
            }
            assert_eq!(child_token.root.owner.strong_count(), 0);
            assert!(PARENT_CALL.with(|slot| slot.borrow().is_none()));
            assert!(CHILD_WRITE.with(|slot| slot.borrow().is_none()));
        } else {
            drop(parent.call.take());
            let error = ancestor.retirement_error().unwrap();
            *expected.lock().unwrap() = Some(error.clone());
            assert_eq!(parent.body_drops.load(Ordering::SeqCst), 1);
            assert_eq!(parent.stack_drops.load(Ordering::SeqCst), 0);
            assert_eq!(
                child_wakes.0.load(Ordering::SeqCst),
                1,
                "cross-Store native descendant was not woken"
            );
            let result = child
                .call
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut Context::from_waker(&child_waker));
            let Poll::Ready(Err(child_error)) = result else {
                panic!("native child ignored ancestor cancellation");
            };
            assert!(std::ptr::eq(
                child_error.downcast_ref::<std::io::Error>().unwrap(),
                error.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert!(exited.load(Ordering::SeqCst));
            assert!(child_token.root.control.settled.get());
            assert_eq!(child_token.root.owner.strong_count(), 0);
            assert!(child_token.root.control.native_edges.borrow().is_empty());
            assert!(std::ptr::eq(
                child_token
                    .retirement_error()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap(),
                error.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert_eq!(child.caught.load(Ordering::SeqCst), 1);
            if let Some(guard) = unmanaged {
                // The unmanaged caller explicitly awaited the descendant's
                // physical settlement before continuing/exiting its own frame.
                assert_eq!(child.stack_drops.load(Ordering::SeqCst), 1);
                assert_eq!(parent.stack_drops.load(Ordering::SeqCst), 0);
                assert!(std::ptr::eq(
                    guard
                        .retirement_error()
                        .unwrap()
                        .downcast_ref::<std::io::Error>()
                        .unwrap(),
                    error.downcast_ref::<std::io::Error>().unwrap()
                ));
                // The unmanaged scope is also physically exited before release.
                drop(guard);
            }
            assert_eq!(parent.caught.load(Ordering::SeqCst), 1);
        }
        assert_eq!(parent.stack_drops.load(Ordering::SeqCst), 1);
        assert_eq!(child.stack_drops.load(Ordering::SeqCst), 1);
        assert!(!ancestor.is_current() && !child_token.is_current());
        assert!(unsafe { ancestor.enter_descendant_frame() }.is_err());
        assert!(unsafe { child_token.enter_descendant_frame() }.is_err());
        for root in [&parent, &child] {
            let continued = futures::executor::block_on(
                root.instance
                    .exports
                    .get_function("continued")
                    .unwrap()
                    .call_async(&root.store, vec![]),
            )
            .unwrap();
            assert_eq!(
                continued[0].i32(),
                Some(i32::from(matches!(proof, FrameProof::Resume)))
            );
            let fresh = futures::executor::block_on(
                root.instance
                    .exports
                    .get_function("fresh")
                    .unwrap()
                    .call_async(&root.store, vec![]),
            )
            .unwrap();
            assert_eq!(fresh[0].i32(), Some(9));
        }
        drop(CHILD.with(|slot| slot.borrow_mut().take()));
        drop(ANCESTOR.with(|slot| slot.borrow_mut().take()));
        drop(PARENT_LOAN.with(|slot| slot.borrow_mut().take()));
        drop(CHILD_LOAN.with(|slot| slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[derive(Clone, Copy)]
    enum ChildProof {
        Return,
        LoanCycle,
        TypedReturn,
        Cancel,
        EscapedGuard,
        ForeignRoot,
        Notify,
    }

    #[test]
    fn native_frame_cycles_through_a_real_store_loan_reject_before_binding() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::native_frame_cycles_through_a_real_store_loan_reject_before_binding",
            ChildProof::LoanCycle,
        );
    }

    #[test]
    fn an_unrelated_physical_root_does_not_inherit_a_logical_body_abort() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::an_unrelated_physical_root_does_not_inherit_a_logical_body_abort",
            ChildProof::ForeignRoot,
        );
    }

    #[test]
    fn descendant_notifications_preserve_unrelated_tasks_and_survive_waker_drop_panic() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::descendant_notifications_preserve_unrelated_tasks_and_survive_waker_drop_panic",
            ChildProof::Notify,
        );
    }

    #[test]
    fn a_typed_borrowed_child_converts_results_after_releasing_its_parent_guard() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::a_typed_borrowed_child_converts_results_after_releasing_its_parent_guard",
            ChildProof::TypedReturn,
        );
    }

    #[test]
    fn a_borrowed_child_releases_its_parent_when_its_completed_future_is_retained() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::a_borrowed_child_releases_its_parent_when_its_completed_future_is_retained",
            ChildProof::Return,
        );
    }

    #[test]
    fn an_ancestor_abort_settles_a_retained_child_error_future() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::an_ancestor_abort_settles_a_retained_child_error_future",
            ChildProof::Cancel,
        );
    }

    #[test]
    fn an_escaped_child_guard_delays_both_canceled_stacks_until_its_release() {
        borrowed_child_proof(
            "backend::sys::async_runtime::tests::an_escaped_child_guard_delays_both_canceled_stacks_until_its_release",
            ChildProof::EscapedGuard,
        );
    }

    fn borrowed_child_proof(name: &str, proof: ChildProof) {
        if !runtime_subprocess(name) {
            return;
        }
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        type ChildCall = Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>>>>;
        struct Counter {
            child: Option<Function>,
            value: i32,
        }
        thread_local! {
            static CHILD: RefCell<Option<ChildCall>> = const { RefCell::new(None) };
            static PARENT_ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
            static CHILD_ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
            static CHILD_GUARD: RefCell<Option<crate::AsyncFunctionEnvHandleMut<Counter>>> = const { RefCell::new(None) };
            static PARENT_CALL: RefCell<Option<ChildCall>> = const { RefCell::new(None) };
            static UNRELATED: RefCell<Option<ChildCall>> = const { RefCell::new(None) };
            static CHILD_WAKER: RefCell<Option<Waker>> = const { RefCell::new(None) };
        }
        struct ParentWakePanic(u32);
        struct ParentWake(Arc<AtomicUsize>);
        impl std::task::Wake for ParentWake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Drop for ParentWake {
            fn drop(&mut self) {
                std::panic::panic_any(ParentWakePanic(98));
            }
        }
        struct ChildWake(Arc<AtomicUsize>);
        impl std::task::Wake for ChildWake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
                UNRELATED.with(|slot| {
                    if let Some(call) = slot.borrow_mut().as_mut() {
                        let waker = futures::task::noop_waker();
                        assert!(
                            call.as_mut()
                                .poll(&mut Context::from_waker(&waker))
                                .is_pending(),
                            "inline descendant wake poisoned an unrelated owned task"
                        );
                    }
                });
            }
        }
        struct OtherWake(Arc<AtomicUsize>);
        impl std::task::Wake for OtherWake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct StackProbe(Arc<AtomicUsize>);
        impl Drop for StackProbe {
            fn drop(&mut self) {
                CHILD_ROOT.with(|slot| {
                    assert!(
                        slot.borrow().as_ref().unwrap().control.settled.get(),
                        "parent resumed or retired before its child physically settled"
                    )
                });
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            stack: Arc<AtomicUsize>,
            continued: Arc<AtomicUsize>,
            caught: Arc<AtomicUsize>,
            expected: Arc<Mutex<Option<RuntimeError>>>,
        }
        let stack_drops = Arc::new(AtomicUsize::new(0));
        let continued = Arc::new(AtomicUsize::new(0));
        let caught = Arc::new(AtomicUsize::new(0));
        let expected = Arc::new(Mutex::new(None::<RuntimeError>));
        let foreign_completed = Arc::new(AtomicUsize::new(0));
        let child_wakes = Arc::new(AtomicUsize::new(0));
        let other_wakes = Arc::new(AtomicUsize::new(0));
        let foreign_result = foreign_completed.clone();
        let expected_from_foreign = expected.clone();
        let mut store = Store::default();
        let counter = crate::FunctionEnv::new(
            &mut store,
            Counter {
                child: None,
                value: 0,
            },
        );
        let parent = crate::FunctionEnv::new(
            &mut store,
            Parent {
                leaf: None,
                stack: stack_drops.clone(),
                continued: continued.clone(),
                caught: caught.clone(),
                expected: expected.clone(),
            },
        );
        let child_wait = Function::new_typed_with_env_async(
            &mut store,
            &counter,
            move |env: crate::AsyncFunctionEnvMut<Counter>| {
                CHILD_ROOT.with(|slot| {
                    assert!(
                        slot.borrow_mut()
                            .replace(CoroutineContext::current_root().unwrap())
                            .is_none()
                    )
                });
                let foreign_result = foreign_result.clone();
                let expected_from_foreign = expected_from_foreign.clone();
                async move {
                    let mut write = env.write().await;
                    write.data_mut().value = 42;
                    if matches!(proof, ChildProof::EscapedGuard) {
                        CHILD_GUARD
                            .with(|slot| assert!(slot.borrow_mut().replace(write).is_none()));
                        return Ok::<i32, RuntimeError>(42);
                    }
                    drop(write);
                    if matches!(proof, ChildProof::Cancel | ChildProof::Notify) {
                        return std::future::pending().await;
                    }
                    let mut first = true;
                    std::future::poll_fn(|cx| {
                        if std::mem::take(&mut first) {
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        } else {
                            Poll::Ready(())
                        }
                    })
                    .await;
                    if matches!(proof, ChildProof::ForeignRoot) {
                        let mut foreign = Store::default();
                        let expected = expected_from_foreign.clone();
                        let stop = Function::new_typed(&mut foreign, move || -> i32 {
                            drop(PARENT_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
                            let error = PARENT_ROOT.with(|slot| {
                                slot.borrow().as_ref().unwrap().control.error().unwrap()
                            });
                            *expected.lock().unwrap() = Some(error);
                            42
                        });
                        let result = stop
                            .call_async(&foreign.into_async(), vec![])
                            .await
                            .unwrap();
                        assert_eq!(result[0].i32(), Some(42));
                        foreign_result.fetch_add(1, Ordering::SeqCst);
                        return std::future::pending().await;
                    }
                    Ok(42)
                }
            },
        );
        let parent_wait = Function::new_typed_with_env_async(
            &mut store,
            &counter,
            move |env: crate::AsyncFunctionEnvMut<Counter>| async move {
                let (child, typed) = {
                    let read = env.read().await;
                    let child = read.data().child.clone().unwrap();
                    let typed = child.typed::<(), i32>(&read).unwrap();
                    (child, typed)
                };
                let store = env.as_store_async();
                let call: ChildCall = if matches!(proof, ChildProof::TypedReturn) {
                    Box::pin(async move {
                        let value = typed.call_async(&store).await?;
                        Ok(vec![Value::I32(value)].into_boxed_slice())
                    })
                } else {
                    Box::pin(child.call_async(&store, vec![]))
                };
                CHILD.with(|slot| assert!(slot.borrow_mut().replace(call).is_none()));
                let values = std::future::poll_fn(|cx| {
                    let override_waker = CHILD_WAKER.with(|slot| slot.borrow().clone());
                    CHILD.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let call = slot.as_mut().unwrap();
                        match override_waker {
                            Some(waker) => call.as_mut().poll(&mut Context::from_waker(&waker)),
                            None => call.as_mut().poll(cx),
                        }
                    })
                })
                .await?;
                // Keep the completed child Future in CHILD while this host body and
                // its parent native stack continue. Completion must release its gate.
                Ok::<i32, RuntimeError>(values[0].i32().unwrap())
            },
        );
        let bridge = Function::new_typed_with_env(
            &mut store,
            &parent,
            |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                PARENT_ROOT.with(|slot| {
                    assert!(
                        slot.borrow_mut()
                            .replace(CoroutineContext::current_root().unwrap())
                            .is_none()
                    )
                });
                let (data, mut store) = env.data_and_store_mut();
                let _stack = StackProbe(data.stack.clone());
                match data.leaf.as_ref().unwrap().call(&mut store) {
                    Ok(value) => {
                        data.continued.fetch_add(1, Ordering::SeqCst);
                        Ok(value)
                    }
                    Err(error) => {
                        assert!(error.is_host_abort());
                        let expected = data.expected.lock().unwrap();
                        assert!(std::ptr::eq(
                            error.downcast_ref::<std::io::Error>().unwrap(),
                            expected
                                .as_ref()
                                .unwrap()
                                .downcast_ref::<std::io::Error>()
                                .unwrap()
                        ));
                        data.caught.fetch_add(1, Ordering::SeqCst);
                        // Deliberately catch cancellation; the post-body veto must
                        // prevent the guest's catch_all or following instruction.
                        Ok(42)
                    }
                }
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "bridge" (func $bridge (result i32)))
            (import "proof" "parent_wait" (func $parent_wait (result i32)))
            (import "proof" "child_wait" (func $child_wait (result i32)))
            (global $continued (mut i32) (i32.const 0))
            (func (export "run") (result i32)
                (block $caught (try_table (catch_all $caught)
                    call $bridge i32.const 1 global.set $continued return))
                i32.const 2 global.set $continued i32.const 0)
            (func (export "parent_leaf") (result i32) call $parent_wait)
            (func (export "child") (result i32) call $child_wait)
            (func (export "spin") (result i32) (loop $again br $again) i32.const 0)
            (func (export "continued") (result i32) global.get $continued)
            (func (export "fresh") (result i32) i32.const 9))"#,
        )
        .unwrap();
        let instance = Instance::new(&mut store, &module, &crate::imports! { "proof" => { "bridge" => bridge, "parent_wait" => parent_wait, "child_wait" => child_wait } }).unwrap();
        parent.as_mut(&mut store).leaf = Some(
            instance
                .exports
                .get_typed_function(&store, "parent_leaf")
                .unwrap(),
        );
        counter.as_mut(&mut store).child =
            Some(instance.exports.get_function("child").unwrap().clone());
        let store = store.into_async();
        let mut call: ChildCall = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let mut waker = if matches!(proof, ChildProof::Notify) {
            CHILD_WAKER.with(|slot| {
                *slot.borrow_mut() = Some(Waker::from(Arc::new(ChildWake(child_wakes.clone()))))
            });
            Waker::from(Arc::new(ParentWake(Arc::new(AtomicUsize::new(0)))))
        } else {
            futures::task::noop_waker()
        };
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        let child_root = CHILD_ROOT.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert!(
            child_root
                .owner
                .upgrade()
                .unwrap()
                .stack
                .borrow()
                .suspended_stores
                .owns_root()
        );
        if matches!(proof, ChildProof::LoanCycle) {
            let parent = PARENT_ROOT.with(|slot| slot.borrow().as_ref().unwrap().control.clone());
            assert!(NativeFrameBinding::enter(parent.clone(), &child_root.control).is_err());
            assert!(parent.native_edges.borrow().is_empty());
        }
        if matches!(proof, ChildProof::Notify) {
            let mut other: ChildCall = Box::pin(
                instance
                    .exports
                    .get_function("fresh")
                    .unwrap()
                    .call_async(&store, vec![]),
            );
            let other_waker = Waker::from(Arc::new(OtherWake(other_wakes.clone())));
            assert!(
                other
                    .as_mut()
                    .poll(&mut Context::from_waker(&other_waker))
                    .is_pending()
            );
            UNRELATED.with(|slot| *slot.borrow_mut() = Some(other));
            let old = std::mem::replace(&mut waker, futures::task::noop_waker());
            drop(old); // only the parent's registry entry now owns ParentWake
        }
        if matches!(
            proof,
            ChildProof::Return | ChildProof::TypedReturn | ChildProof::LoanCycle
        ) {
            let result = futures::executor::block_on(call.as_mut()).unwrap();
            assert_eq!(result[0].i32(), Some(42));
            if matches!(proof, ChildProof::TypedReturn) {
                // The typed wrapper finished its inner native future before
                // reacquiring the parent's loan for result conversion.
                assert_eq!(child_root.owner.strong_count(), 0);
            } else {
                let child = child_root
                    .owner
                    .upgrade()
                    .expect("completed child Future still retained");
                assert!(child.stack.borrow().coroutine.is_none());
                assert!(!child.stack.borrow().suspended_stores.owns_root());
                assert!(!child.control.retained.get());
            }
            assert!(child_root.control.settled.get());
            assert_eq!(continued.load(Ordering::SeqCst), 1);
            drop(call);
        } else if matches!(proof, ChildProof::ForeignRoot) {
            PARENT_CALL.with(|slot| assert!(slot.borrow_mut().replace(call).is_none()));
            let result = CHILD.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
            });
            let Poll::Ready(Err(error)) = result else {
                panic!("logical body missed ancestor cancellation after unrelated root returned");
            };
            assert_eq!(foreign_completed.load(Ordering::SeqCst), 1);
            assert!(error.is_host_abort());
            assert!(std::ptr::eq(
                error.downcast_ref::<std::io::Error>().unwrap(),
                expected
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
            ));
            assert_eq!(child_root.owner.strong_count(), 0);
            assert_eq!(caught.load(Ordering::SeqCst), 1);
            assert_eq!(continued.load(Ordering::SeqCst), 0);
        } else {
            let mut escaped = CHILD_GUARD.with(|slot| slot.borrow_mut().take());
            let mut before_cancel = escaped.as_mut().map(AsStoreMut::as_store_mut);
            if matches!(proof, ChildProof::Notify) {
                let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(call)))
                    .expect_err("parent Waker destructor must panic after notifying descendants");
                assert_eq!(panic.downcast_ref::<ParentWakePanic>().unwrap().0, 98);
                assert_eq!(child_wakes.load(Ordering::SeqCst), 1);
                assert_eq!(other_wakes.load(Ordering::SeqCst), 0);
            } else {
                drop(call);
            }
            let original =
                PARENT_ROOT.with(|slot| slot.borrow().as_ref().unwrap().control.error().unwrap());
            *expected.lock().unwrap() = Some(original.clone());
            // Remove the test metadata owner too: the child must preserve its
            // terminal error when the parent's weak control edge later expires.
            drop(PARENT_ROOT.with(|slot| slot.borrow_mut().take()));
            if let Some(store) = &mut before_cancel {
                let error = instance
                    .exports
                    .get_function("spin")
                    .unwrap()
                    .call(store, &[])
                    .unwrap_err();
                assert!(std::ptr::eq(
                    error.downcast_ref::<std::io::Error>().unwrap(),
                    original.downcast_ref::<std::io::Error>().unwrap()
                ));
            }
            CHILD_GUARD.with(|slot| *slot.borrow_mut() = escaped);
            let result = CHILD.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
            });
            let Poll::Ready(Err(error)) = result else {
                panic!("ancestor cancellation did not terminate child");
            };
            assert!(std::ptr::eq(
                error.downcast_ref::<std::io::Error>().unwrap(),
                original.downcast_ref::<std::io::Error>().unwrap()
            ));
            if matches!(proof, ChildProof::EscapedGuard) {
                assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
                let mut foreign = Store::default();
                let release = Function::new_typed(&mut foreign, || -> i32 {
                    drop(CHILD_GUARD.with(|slot| slot.borrow_mut().take().unwrap()));
                    42
                });
                let result =
                    futures::executor::block_on(release.call_async(&foreign.into_async(), vec![]))
                        .unwrap();
                assert_eq!(result[0].i32(), Some(42));
            }
            // CHILD still owns its completed error Future. Its Root must have
            // been released on terminal poll, independently of Future Drop.
            assert!(CHILD.with(|slot| slot.borrow().is_some()));
            assert_eq!(child_root.owner.strong_count(), 0);
            assert!(
                child_root
                    .control
                    .parent
                    .as_ref()
                    .unwrap()
                    .upgrade()
                    .is_none()
            );
            assert!(std::ptr::eq(
                child_root
                    .control
                    .error()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap(),
                original.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert_eq!(caught.load(Ordering::SeqCst), 1);
            assert_eq!(continued.load(Ordering::SeqCst), 0);
        }
        assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
        if matches!(proof, ChildProof::Notify) {
            let call = UNRELATED.with(|slot| slot.borrow_mut().take().unwrap());
            let result = futures::executor::block_on(call).unwrap();
            assert_eq!(result[0].i32(), Some(9));
            drop(CHILD_WAKER.with(|slot| slot.borrow_mut().take()));
        }
        let result = futures::executor::block_on(
            instance
                .exports
                .get_function("fresh")
                .unwrap()
                .call_async(&store, vec![]),
        )
        .unwrap();
        assert_eq!(result[0].i32(), Some(9));
        let result = futures::executor::block_on(
            instance
                .exports
                .get_function("continued")
                .unwrap()
                .call_async(&store, vec![]),
        )
        .unwrap();
        assert_eq!(
            result[0].i32(),
            Some(i32::from(matches!(
                proof,
                ChildProof::Return | ChildProof::TypedReturn | ChildProof::LoanCycle
            )))
        );
        drop(CHILD.with(|slot| slot.borrow_mut().take()));
        drop(CHILD_ROOT.with(|slot| slot.borrow_mut().take()));
        drop(PARENT_ROOT.with(|slot| slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[test]
    fn registry_callbacks_can_reenter_while_the_current_waker_changes() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::registry_callbacks_can_reenter_while_the_current_waker_changes",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct ReentrantWake {
            store_id: StoreId,
            clones: AtomicUsize,
            wakes: AtomicUsize,
            drops: AtomicUsize,
        }
        impl ReentrantWake {
            fn touch(&self, count: &AtomicUsize) {
                // Every user callback can inspect its own Store's registry.
                // A live DashMap entry around any of these calls deadlocks.
                drop(ASYNC_CALL_FUTURE_WAKERS.get(&self.store_id));
                count.fetch_add(1, Ordering::SeqCst);
            }
            fn waker(self: &Arc<Self>) -> Waker {
                static VTABLE: RawWakerVTable =
                    RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);
                unsafe fn clone_waker(data: *const ()) -> RawWaker {
                    // Borrow the existing owned Arc without consuming its raw
                    // reference; return a separately incremented owned clone.
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<ReentrantWake>())
                    });
                    state.touch(&state.clones);
                    RawWaker::new(Arc::into_raw(Arc::clone(&state)).cast(), &VTABLE)
                }
                unsafe fn wake(data: *const ()) {
                    let state = unsafe { Arc::from_raw(data.cast::<ReentrantWake>()) };
                    state.touch(&state.wakes);
                }
                unsafe fn wake_by_ref(data: *const ()) {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<ReentrantWake>())
                    });
                    state.touch(&state.wakes);
                }
                unsafe fn drop_waker(data: *const ()) {
                    let state = unsafe { Arc::from_raw(data.cast::<ReentrantWake>()) };
                    state.touch(&state.drops);
                }
                // Each RawWaker owns exactly the Arc cloned here or by its
                // clone vtable entry; wake/drop consume that one reference.
                unsafe {
                    Waker::from_raw(RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE))
                }
            }
        }
        let mut store = Store::default();
        let pending = Function::new_async(&mut store, crate::FunctionType::new([], []), |_| {
            std::future::pending::<Result<Vec<Value>, RuntimeError>>()
        });
        let store = store.into_async();
        let make_state = || {
            Arc::new(ReentrantWake {
                store_id: store.id,
                clones: AtomicUsize::new(0),
                wakes: AtomicUsize::new(0),
                drops: AtomicUsize::new(0),
            })
        };
        let first = make_state();
        let second = make_state();
        let first_waker = first.waker();
        let second_waker = second.waker();
        let mut call = Box::pin(AsyncCallFuture::new(pending.into_sys(), store, vec![]));
        let poll = |call: &mut Pin<Box<AsyncCallFuture>>, waker: &Waker| {
            assert!(
                call.as_mut()
                    .poll(&mut Context::from_waker(waker))
                    .is_pending()
            );
        };
        poll(&mut call, &first_waker);
        assert_eq!(first.clones.load(Ordering::SeqCst), 1);
        poll(&mut call, &first_waker);
        assert_eq!(
            first.clones.load(Ordering::SeqCst),
            1,
            "same identity needs no clone"
        );
        poll(&mut call, &second_waker);
        assert_eq!(second.clones.load(Ordering::SeqCst), 1);
        assert_eq!(first.drops.load(Ordering::SeqCst), 1);
        notify_pending_futures_of_interrupt(second.store_id);
        assert_eq!(first.wakes.load(Ordering::SeqCst), 0);
        assert_eq!(second.wakes.load(Ordering::SeqCst), 1);
        assert_eq!(
            second.clones.load(Ordering::SeqCst),
            1,
            "snapshot clones only Arc"
        );
        drop(call);
        assert_eq!(second.drops.load(Ordering::SeqCst), 1);
        assert!(!ASYNC_CALL_FUTURE_WAKERS.contains_key(&second.store_id));
        drop(first_waker);
        drop(second_waker);
        assert_eq!(first.drops.load(Ordering::SeqCst), 2);
        assert_eq!(second.drops.load(Ordering::SeqCst), 2);
        assert_eq!(Arc::strong_count(&first), 1);
        assert_eq!(Arc::strong_count(&second), 1);
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    fn runtime_subprocess(test_name: &str) -> bool {
        const CHILD: &str = "WASMER_NATIVE_ASYNC_RUNTIME_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Like async_sync_borrow, isolate invalid native stack transitions
            // in an owned, bounded child process.
            struct OwnedChild(std::process::Child);
            impl Drop for OwnedChild {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let mut child = OwnedChild(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", test_name, "--nocapture"])
                    .env(CHILD, "1")
                    .spawn()
                    .unwrap(),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                match child.0.try_wait() {
                    Ok(Some(status)) => {
                        assert!(status.success(), "native runtime proof failed: {status}");
                        return false;
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    result => panic!("native runtime proof did not finish: {result:?}"),
                }
            }
        }

        true
    }

    #[test]
    fn cancellation_destructors_cannot_yield_through_another_root() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::cancellation_destructors_cannot_yield_through_another_root",
        ) {
            return;
        }

        type HiddenCaller = (Function, crate::TypedFunction<(), i32>, StoreAsync);
        thread_local! {
            static HIDDEN_CALLER: RefCell<Option<HiddenCaller>> = const { RefCell::new(None) };
        }
        struct Probe(Rc<std::cell::Cell<usize>>);
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(CoroutineContext::get_current().is_none());
                assert!(matches!(
                    block_on_host_future(std::future::pending()),
                    Err(AsyncRuntimeError::YieldOutsideAsyncContext)
                ));
                let caller = HIDDEN_CALLER.with(|slot| {
                    slot.borrow().as_ref().map(|(function, typed, store)| {
                        (function.clone(), typed.clone(), store.store())
                    })
                });
                if let Some((function, typed, store)) = caller {
                    // The foreign caller's original write guard is hidden, not
                    // released. A destructor must reject before waiting on it.
                    let mut call = Box::pin(function.call_async(&store, vec![]));
                    let waker = futures::task::noop_waker();
                    let Poll::Ready(Err(error)) =
                        call.as_mut().poll(&mut Context::from_waker(&waker))
                    else {
                        panic!("retirement attempted to wait on its hidden caller");
                    };
                    assert!(error.is_host_abort());
                    let mut call = Box::pin(typed.call_async(&store));
                    let Poll::Ready(Err(error)) =
                        call.as_mut().poll(&mut Context::from_waker(&waker))
                    else {
                        panic!("typed retirement attempted to wait on its hidden caller");
                    };
                    assert!(error.is_host_abort());
                }
                self.0.set(self.0.get() + 1);
            }
        }
        struct PendingBody(Probe);
        impl Future for PendingBody {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                let _ = &self.0;
                Poll::Pending
            }
        }
        struct RegistryWake {
            store_id: StoreId,
            drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
            wakes: std::sync::atomic::AtomicUsize,
        }
        impl std::task::Wake for RegistryWake {
            fn wake(self: std::sync::Arc<Self>) {
                self.wakes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        impl Drop for RegistryWake {
            fn drop(&mut self) {
                assert!(CoroutineContext::get_current().is_none());
                // A Waker destructor can reenter the registry for its own key.
                // Removing it must release the DashMap entry before this call.
                assert!(ASYNC_CALL_FUTURE_WAKERS.get(&self.store_id).is_none());
                self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let drops = Rc::new(std::cell::Cell::new(0));
        let mut first_store = Store::default();
        let _data = crate::FunctionEnv::new(&mut first_store, ());
        // This Store-owned closure captures a second destructor probe without
        // requiring a Send implementation for the thread-local test state.
        let store_probe = Probe(drops.clone());
        let _owner = Function::new_async(
            &mut first_store,
            crate::FunctionType::new([], []),
            move |_| {
                let _ = &store_probe;
                async { Ok(vec![]) }
            },
        );
        let count = drops.clone();
        let wait = Function::new_async(
            &mut first_store,
            crate::FunctionType::new([], []),
            move |_| PendingBody(Probe(count.clone())),
        );
        let module = Module::new(
            &first_store,
            r#"(module (import "proof" "wait" (func $wait))
                (func (export "run") call $wait))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut first_store,
            &module,
            &crate::imports! { "proof" => { "wait" => wait } },
        )
        .unwrap();
        let mut first = Box::pin(AsyncCallFuture::new(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .as_sys()
                .clone(),
            first_store.into_async(),
            vec![],
        ));
        let waker_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let waker = Waker::from(std::sync::Arc::new(RegistryWake {
            store_id: first.store().store_id(),
            drops: waker_drops.clone(),
            wakes: std::sync::atomic::AtomicUsize::new(0),
        }));
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);
        assert_eq!(drops.get(), 0);

        let first = Rc::new(RefCell::new(Some(first)));
        let mut second_store = Store::default();
        let caller_entries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = caller_entries.clone();
        let fresh = Function::new_typed(&mut second_store, move || -> i32 {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            9
        });
        let cancel = Function::new_async(
            &mut second_store,
            crate::FunctionType::new([], []),
            move |_| {
                let current = CoroutineContext::get_current().expect("second root active");
                drop(first.borrow_mut().take().unwrap());
                assert_eq!(CoroutineContext::get_current(), Some(current));
                async { Ok(vec![]) }
            },
        );
        let module = Module::new(
            &second_store,
            r#"(module (import "proof" "cancel" (func $cancel))
                (func (export "run") (result i32) call $cancel i32.const 42))"#,
        )
        .unwrap();
        let second = Instance::new(
            &mut second_store,
            &module,
            &crate::imports! { "proof" => { "cancel" => cancel } },
        )
        .unwrap();
        let fresh_typed = fresh.typed::<(), i32>(&second_store).unwrap();
        let second_store = second_store.into_async();
        HIDDEN_CALLER.with(|slot| {
            assert!(
                slot.borrow_mut()
                    .replace((fresh.clone(), fresh_typed, second_store.store()))
                    .is_none()
            );
        });
        let result = futures::executor::block_on(
            second
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&second_store, vec![]),
        )
        .unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(drops.get(), 2);
        assert_eq!(waker_drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(caller_entries.load(std::sync::atomic::Ordering::SeqCst), 0);
        HIDDEN_CALLER.with(|slot| drop(slot.borrow_mut().take()));
        let result = futures::executor::block_on(fresh.call_async(&second_store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(9));
        assert_eq!(caller_entries.load(std::sync::atomic::Ordering::SeqCst), 1);
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_distinct_store_async_call_rejects_recursive_poll_without_starting() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let entered = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let count = entered.clone();
        let function = Function::new_typed(&mut store, move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let store = store.into_async();
        let mut call = Box::pin(function.call_async(&store, vec![]));
        let other = Store::default().into_async();
        let root = StoreContext::install_async_with_id(
            other.inner.try_write().unwrap(),
            StoreContext::new_installation_id(),
        );
        let waker = futures::task::noop_waker();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }))
            .is_err()
        );
        assert_eq!(entered.load(Ordering::SeqCst), 0);
        {
            let detached = StoreContext::detach_current();
            assert!(StoreContext::is_empty());
            assert!(!StoreContext::async_root_admissible());
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    call.as_mut().poll(&mut Context::from_waker(&waker))
                }))
                .is_err()
            );
            drop(detached);
            assert_eq!(entered.load(Ordering::SeqCst), 0);
        }
        assert_eq!(
            unsafe { StoreContext::get_current(other.id) }
                .as_ref()
                .objects()
                .id(),
            other.id
        );
        drop(root);
        // Rejection did not poll or poison the exact backend invocation.
        assert!(futures::executor::block_on(call).unwrap().is_empty());
        assert_eq!(entered.load(Ordering::SeqCst), 1);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_engine_coroutine_restores_context_before_the_next_call() {
        let mut store = Store::default();
        let panic = Function::new_typed(&mut store, || -> () {
            panic!("synchronous callback panic");
        });
        let ready = Function::new_async(&mut store, crate::FunctionType::new([], []), |_| async {
            Ok(vec![])
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "panic" (func $panic))
            (import "proof" "ready" (func $ready))
            (func (export "bad") call $panic)
            (func (export "next") (result i32) call $ready i32.const 7))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "panic" => panic, "ready" => ready } },
        )
        .unwrap();
        let store = store.into_async();
        let bad = instance.exports.get_function("bad").unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures::executor::block_on(bad.call_async(&store, vec![]))
            }))
            .is_err()
        );
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
        let next = instance.exports.get_function("next").unwrap();
        let result = futures::executor::block_on(next.call_async(&store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(7));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_nested_coroutine_preserves_its_live_parent_context() {
        let mut parent = Coroutine::new(
            |yielder: &Yielder<AsyncResume, AsyncYield>, _: AsyncResume| {
                let context = CoroutineContext::new(yielder, StoreContext::new_installation_id());
                context.enter();
                let mut child = Coroutine::new(
                    |yielder: &Yielder<AsyncResume, AsyncYield>,
                     _: AsyncResume|
                     -> Result<Box<[Value]>, RuntimeError> {
                        let context =
                            CoroutineContext::new(yielder, StoreContext::new_installation_id());
                        context.enter();
                        panic!("nested coroutine panic");
                    },
                );
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let boundary = CoroutineBoundary::new();
                    let _ = child.resume(AsyncResume::Start);
                    boundary.returned();
                }));
                let retained =
                    CURRENT_CONTEXT.with(|current| *current.borrow() == vec![&context as *const _]);
                context.leave();
                assert!(caught.is_err());
                assert!(retained);
                Ok::<_, RuntimeError>(Vec::<Value>::new().into_boxed_slice())
            },
        );
        let boundary = CoroutineBoundary::new();
        let result = parent.resume(AsyncResume::Start);
        boundary.returned();
        assert!(matches!(result, CoroutineResult::Return(Ok(_))));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }
}
