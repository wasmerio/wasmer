//! Explicit context switches retain native invocation futures on the owning runner.

use crate::asyncify::Access;
use std::{
    collections::BTreeMap,
    future::{Future, poll_fn},
    sync::{Arc, Mutex, MutexGuard},
    task::Poll,
};
use wasixv2_syscalls::{Control, Errno, LocalFuture, SysResult, Syscalls, TaskId};
use wasmer::{AsStoreAsync, AsyncFunctionEnvMut, Function, FunctionEnv, RuntimeError, StoreAsync};

use crate::{Entry, ExecEnv};

#[cfg(feature = "js")]
pub(crate) mod jspi;

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Active,
    Suspended,
}

pub(crate) struct Shared {
    task: TaskId,
    owner: std::thread::ThreadId,
    state: Mutex<State>,
}
struct State {
    next: u64,
    active: u64,
    current: u64,
    contexts: BTreeMap<u64, Status>,
    pending: Vec<(u64, Function)>,
    closed: bool,
    original_error: Option<(u64, RuntimeError)>,
    terminal: Option<Result<(), RuntimeError>>,
    next_error: u64,
    #[cfg(feature = "js")]
    jspi: jspi::State,
}
impl Shared {
    fn new(task: TaskId) -> Arc<Self> {
        Arc::new(Self {
            task,
            owner: std::thread::current().id(),
            state: Mutex::new(State {
                next: 1,
                active: 0,
                current: 0,
                contexts: BTreeMap::from([(0, Status::Active)]),
                pending: Vec::new(),
                closed: false,
                original_error: None,
                terminal: None,
                next_error: 0,
                #[cfg(feature = "js")]
                jspi: Default::default(),
            }),
        })
    }

    pub(crate) fn task(&self) -> TaskId {
        self.task
    }

    pub(crate) fn current(&self) -> u64 {
        lock(&self.state).current
    }

    pub(crate) fn error_order(&self) -> u64 {
        let mut state = lock(&self.state);
        let order = state.next_error;
        state.next_error = order.checked_add(1).expect("context error order exhausted");
        order
    }

    pub(crate) fn retired_error(&self, id: u64, order: u64, error: RuntimeError) {
        #[cfg(feature = "js")]
        if jspi::is_cancellation(self, id, &error) {
            return;
        }
        let previous = {
            let mut state = lock(&self.state);
            if (state.closed || !state.contexts.contains_key(&id))
                && state
                    .original_error
                    .as_ref()
                    .is_none_or(|(previous, _)| order < *previous)
            {
                state.original_error.replace((order, error))
            } else {
                Some((order, error))
            }
        };
        drop(previous);
    }

    /// The attachment owner calls this only after physical roots and the Store
    /// fence have settled. Release every transport clone before owned downcasts.
    pub(crate) fn resolve(&self, result: Result<(), RuntimeError>) -> Result<(), RuntimeError> {
        let (terminal, retired) = {
            let mut state = lock(&self.state);
            (state.terminal.take(), state.original_error.take())
        };
        if let Some(Err(error)) = terminal {
            // Cooperation can fail while logical context retirement is pending.
            // The already-owned guest outcome remains primary in that case.
            drop(result);
            drop(retired);
            return Err(error);
        }
        match result {
            Err(error) => {
                drop(retired);
                Err(error)
            }
            Ok(()) => retired.map_or(Ok(()), |(_, error)| Err(error)),
        }
    }

    fn close(&self) {
        self.close_with_outcome(None);
    }

    fn close_with_outcome(&self, error: Option<(u64, RuntimeError)>) {
        let (pending, previous) = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.contexts.clear();
            let previous = error.and_then(|(order, error)| {
                if state
                    .original_error
                    .as_ref()
                    .is_none_or(|(old, _)| order < *old)
                {
                    state.original_error.replace((order, error))
                } else {
                    Some((order, error))
                }
            });
            (std::mem::take(&mut state.pending), previous)
        };
        // Exact custody is committed before arbitrary payload/Function drops or
        // wake callbacks can unwind. No user destruction runs under the lock.
        drop(previous);
        drop(pending);
        #[cfg(feature = "js")]
        jspi::changed(self);
    }

    fn check(&self) -> SysResult<()> {
        let state = lock(&self.state);
        if self.owner != std::thread::current().id()
            || state.closed
            || !state.contexts.contains_key(&state.current)
        {
            return Err(Errno::EOPNOTSUPP.into());
        }
        Ok(())
    }

    pub(crate) fn reserve(&self) -> SysResult<u64> {
        self.check()?;
        let mut state = lock(&self.state);
        let id = state.next;
        state.next = id.checked_add(1).ok_or(Errno::EOVERFLOW)?;
        Ok(id)
    }

    pub(crate) fn create(&self, id: u64, function: Function) -> SysResult<()> {
        self.check()?;
        let mut state = lock(&self.state);
        state.contexts.insert(id, Status::Suspended);
        state.pending.push((id, function));
        drop(state);
        #[cfg(feature = "js")]
        jspi::changed(self);
        Ok(())
    }

    pub(crate) async fn switch(&self, id: u64) -> SysResult<()> {
        self.check()?;
        let current = {
            let mut state = lock(&self.state);
            let current = state.current;
            if id == current {
                return Ok(());
            }
            if state.contexts.get(&id) != Some(&Status::Suspended) {
                return Err(Errno::EINVAL.into());
            }
            state.contexts.insert(current, Status::Suspended);
            state.contexts.insert(id, Status::Active);
            state.active = id;
            current
        };
        #[cfg(feature = "js")]
        jspi::changed(self);
        poll_fn(|_| {
            let state = lock(&self.state);
            if state.contexts.get(&current) == Some(&Status::Active) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }

    pub(crate) fn destroy(&self, id: u64) -> SysResult<()> {
        self.check()?;
        let mut state = lock(&self.state);
        if id == 0 || id == state.active {
            return Err(Errno::EINVAL.into());
        }
        state.contexts.remove(&id);
        drop(state);
        #[cfg(feature = "js")]
        jspi::changed(self);
        Ok(())
    }
}

/// Capture before returning a host-import future: outside the native coroutine
/// the exact retained-borrow boundary is no longer installed in TLS.
pub fn context_operations_supported() -> bool {
    #[cfg(feature = "sys")]
    if wasmer::sys::context_switch_supported() {
        return true;
    }
    #[cfg(feature = "js")]
    {
        return jspi::callback_supported();
    }
    #[cfg(not(feature = "js"))]
    false
}

pub(crate) async fn group<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
) -> SysResult<Arc<Shared>> {
    let guard = env.read().await;
    let data = guard.data();
    let group = data.contexts.upgrade().ok_or(Errno::EOPNOTSUPP)?;
    // A borrowed vfork instance has no context environment until exec.
    if group.task != data.task {
        return Err(Errno::EOPNOTSUPP.into());
    }
    group.check()?;
    Ok(group)
}

/// True before an environment exists, and only in its main context afterward.
/// The kernel checks this snapshot before allocating a copying-fork/vfork child.
pub async fn process_fork_allowed<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>) -> bool {
    env.read().await.data().process_fork_allowed()
}

/// A native error remains attached to the exact logical call while later cleanup
/// awaits the Store. Cancellation may retire that call before its caller receives it.
pub(crate) struct PendingError {
    owner: Option<(std::sync::Weak<Shared>, u64)>,
    error: Option<(u64, RuntimeError)>,
}
impl PendingError {
    pub(crate) fn new(
        owner: Option<(std::sync::Weak<Shared>, u64)>,
        error: Option<&RuntimeError>,
    ) -> Self {
        let error = error.map(|error| {
            let order = owner
                .as_ref()
                .and_then(|(group, _)| group.upgrade())
                .map_or(0, |group| group.error_order());
            (order, error.clone())
        });
        Self { owner, error }
    }
}
impl Drop for PendingError {
    fn drop(&mut self) {
        if let Some((order, error)) = self.error.take()
            && let Some((group, id)) = &self.owner
            && let Some(group) = group.upgrade()
        {
            group.retired_error(*id, order, error);
        }
    }
}

struct Invocation {
    future: Option<LocalFuture<'static, Result<(), RuntimeError>>>,
    cpu: crate::cputime::CpuContext,
    asyncify: crate::asyncify::ContextState,
    system: bool,
    #[cfg(feature = "js")]
    started: bool,
}
impl Drop for Invocation {
    fn drop(&mut self) {
        let future = self.future.take();
        self.cpu.run(&mut self.system, || drop(future));
    }
}

#[derive(Debug, thiserror::Error)]
#[error("a context entry returned")]
pub(crate) struct Returned;

enum Next {
    Changed,
    Completed(u64, Result<(), RuntimeError>),
}

/// Owns each actual root outside ExecEnv. Canceled runner futures transfer these
/// exact owners to the existing finite cleanup scope, without erasing lifetimes.
struct Driver<S: Send + Sync + 'static> {
    group: Arc<Shared>,
    roots: BTreeMap<u64, Invocation>,
    env: FunctionEnv<ExecEnv<S>>,
    store: StoreAsync,
    cpu: crate::cputime::CpuContext,
    installed: u64,
    cleanup: Arc<wasixv2_host::LoaderCleanup>,
    #[cfg(feature = "js")]
    registration: Option<jspi::Registration>,
}
impl<S: Send + Sync + 'static> Driver<S> {
    async fn activate(&mut self, id: u64) {
        if self.installed == id {
            return;
        }
        loop {
            #[cfg(feature = "js")]
            jspi::before_activate(&self.group, id).await;
            let roots = &mut self.roots;
            let previous = self.installed;
            let (activated, retired) = crate::asyncify::Outside {
                env: &self.env,
                store: &self.store,
            }
            .with(|env, _| {
                #[cfg(feature = "js")]
                let mut state = lock(&self.group.state);
                // Store acquisition can yield after the first handoff check.
                // Recheck and install the owner in this same no-await transition.
                #[cfg(feature = "js")]
                if !jspi::may_activate(&state, id) {
                    return (false, None);
                }
                let mut retired = None;
                if let Some(asyncify) = &mut env.asyncify {
                    let old = asyncify.take_context();
                    if let Some(root) = roots.get_mut(&previous) {
                        root.asyncify = old;
                    } else {
                        retired = Some(old);
                    }
                    asyncify.put_context(std::mem::take(&mut roots.get_mut(&id).unwrap().asyncify));
                }
                #[cfg(feature = "js")]
                {
                    state.current = id;
                }
                #[cfg(not(feature = "js"))]
                {
                    lock(&self.group.state).current = id;
                }
                (true, retired)
            })
            .await;
            if activated {
                self.installed = id;
                drop(retired);
                #[cfg(feature = "js")]
                jspi::changed(&self.group);
                return;
            }
        }
    }

    async fn retire(&mut self, id: u64) {
        #[cfg(feature = "js")]
        if self.registration.is_some() {
            jspi::select_retirement(&self.group, id);
        }
        self.activate(id).await;
        #[cfg(feature = "js")]
        if self.registration.is_some() {
            let root = self.roots.get_mut(&id).unwrap();
            if root.started
                && let Some(future) = root.future.as_mut()
            {
                let result =
                    poll_fn(|cx| self.cpu.run(&mut root.system, || future.as_mut().poll(cx))).await;
                if let Err(error) = result
                    && !jspi::is_cancellation(&self.group, id, &error)
                {
                    self.group
                        .retired_error(id, self.group.error_order(), error);
                }
            }
            jspi::settle_context(&self.group, id).await;
            jspi::root_settled(&self.group, id);
        }
        drop(self.roots.remove(&id));
    }

    async fn drain(&mut self) {
        self.group.close();
        // The active physical root may retain an original borrowed Store guard.
        // Retire it before requesting any fresh Store lock for a sibling.
        if self.roots.contains_key(&self.installed) {
            self.retire(self.installed).await;
        }
        let ids = self.roots.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.retire(id).await;
        }
        crate::asyncify::Outside {
            env: &self.env,
            store: &self.store,
        }
        .with(|env, _| {
            if env.contexts.ptr_eq(&Arc::downgrade(&self.group)) {
                env.contexts = Default::default();
            }
        })
        .await;
        #[cfg(feature = "js")]
        jspi::drained(&self.group);
    }
}
impl<S: Send + Sync + 'static> Drop for Driver<S> {
    fn drop(&mut self) {
        #[cfg(feature = "js")]
        let incomplete = self.registration.is_some() && !jspi::is_drained(&self.group);
        #[cfg(not(feature = "js"))]
        let incomplete = false;
        if self.roots.is_empty() && !incomplete {
            return;
        }
        let mut pending = Self {
            group: self.group.clone(),
            roots: std::mem::take(&mut self.roots),
            env: self.env.clone(),
            store: self.store.store(),
            cpu: self.cpu.clone(),
            installed: self.installed,
            cleanup: self.cleanup.clone(),
            #[cfg(feature = "js")]
            registration: self.registration.take(),
        };
        let unwinding = std::thread::panicking();
        let closing =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pending.group.close()));
        // Like AttachmentGuard: a waking callback may panic, but the exact owners
        // transfer before any caught failure escapes this destructor.
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.cleanup.spawn(Box::pin(async move {
                pending.drain().await;
            }));
        }));
        let mut failures: Vec<_> = [closing, spawned]
            .into_iter()
            .filter_map(Result::err)
            .collect();
        if !failures.is_empty() {
            let first = failures.remove(0);
            for failure in failures {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failure)));
            }
            if unwinding {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(first)));
            } else {
                std::panic::resume_unwind(first);
            }
        }
    }
}

/// Explicit switches select one retained root. Pending ordinary I/O never
/// implicitly activates a sibling. The no-JSPI browser lane stays unsupported.
pub(crate) async fn drive<S, G>(
    entry: &Entry<'_, S>,
    attachments: &mut Option<crate::instantiation::AttachmentGuard>,
    guest: G,
) -> Result<(), RuntimeError>
where
    S: Syscalls + Control + Send + Sync + 'static,
    G: Future<Output = Result<(), RuntimeError>> + 'static,
{
    let supported = entry
        .with_store(|env, store| {
            if !env.contexts_supported {
                return false;
            }
            #[cfg(feature = "sys")]
            if store.engine().is_sys() {
                return true;
            }
            #[cfg(feature = "js")]
            if store.engine().is_js() {
                return wasmer::js::async_mode(store) == wasmer::js::AsyncMode::Jspi;
            }
            let _ = store;
            false
        })
        .await;
    if !supported {
        return guest.await;
    }
    #[cfg(feature = "js")]
    let javascript = entry.with_store(|_, store| store.engine().is_js()).await;
    let (group, cpu, cleanup) = entry
        .with_store(|env, _| {
            let group = Shared::new(env.task);
            env.contexts = Arc::downgrade(&group);
            (
                group,
                env.cpu.context(),
                env.cleanup.as_ref().expect("runner cleanup exists").clone(),
            )
        })
        .await;
    if attachments.is_none() {
        *attachments = Some(crate::instantiation::AttachmentGuard::new(
            None,
            &entry.store(),
            cleanup.clone(),
            Default::default(),
        ));
    }
    attachments
        .as_mut()
        .expect("contexts require invocation cleanup custody")
        .retain_contexts(group.clone());
    let mut driver = Driver {
        #[cfg(feature = "js")]
        registration: jspi::register(entry.store().store_id(), &group, cpu.clone(), javascript),
        group,
        roots: BTreeMap::from([(
            0,
            Invocation {
                future: Some(Box::pin(guest)),
                cpu: cpu.clone(),
                asyncify: Default::default(),
                system: false,
                #[cfg(feature = "js")]
                started: false,
            },
        )]),
        env: entry.cx().env.clone(),
        store: entry.store(),
        cpu,
        installed: 0,
        cleanup,
    };
    let result = loop {
        let (active, pending, live) = {
            let mut state = lock(&driver.group.state);
            (
                state.active,
                std::mem::take(&mut state.pending),
                state.contexts.keys().copied().collect::<Vec<_>>(),
            )
        };
        for (id, function) in pending {
            if live.contains(&id) {
                let env = driver.env.clone();
                let store = driver.store.store();
                driver.roots.insert(
                    id,
                    Invocation {
                        future: Some(Box::pin(async move {
                            Entry::new(&env, &store)
                                .call_once(&function, &[])
                                .await
                                .map(drop)
                        })),
                        cpu: driver.cpu.clone(),
                        asyncify: Default::default(),
                        system: false,
                        #[cfg(feature = "js")]
                        started: false,
                    },
                );
            }
        }
        let deleted = driver
            .roots
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for id in deleted {
            driver.retire(id).await;
        }
        if let Some((_, error)) = lock(&driver.group.state).original_error.take() {
            break Err(error);
        }
        driver.activate(active).await;
        let next = poll_fn(|cx| {
            #[cfg(feature = "js")]
            jspi::register_driver(&driver.group, cx.waker());
            let root = driver
                .roots
                .get_mut(&active)
                .expect("active context has a root");
            #[cfg(feature = "js")]
            {
                root.started = true;
            }
            let polled = driver.cpu.run(&mut root.system, || {
                root.future.as_mut().unwrap().as_mut().poll(cx)
            });
            if let Poll::Ready(result) = polled {
                #[cfg(feature = "js")]
                if driver.registration.is_some() {
                    let future = root.future.take();
                    driver.cpu.run(&mut root.system, || drop(future));
                }
                return Poll::Ready(Next::Completed(active, result));
            }
            let state = lock(&driver.group.state);
            if state.active != active
                || !state.pending.is_empty()
                || driver
                    .roots
                    .keys()
                    .any(|id| !state.contexts.contains_key(id))
            {
                Poll::Ready(Next::Changed)
            } else {
                Poll::Pending
            }
        })
        .await;
        match next {
            Next::Changed => continue,
            #[cfg(feature = "js")]
            Next::Completed(id, Err(error)) if jspi::is_cancellation(&driver.group, id, &error) => {
                // This own marker only retires a continuation. The attachment
                // owner resolves its original outcome after every physical call.
                break Ok(());
            }
            Next::Completed(0, result) => break result,
            Next::Completed(_, Err(error)) => break Err(error),
            Next::Completed(_, Ok(())) => break Err(RuntimeError::user(Box::new(Returned))),
        }
    };
    // The attachment guard owns this group through physical settlement, even
    // when cancellation moves the driver's roots into finite cleanup.
    lock(&driver.group.state).terminal = Some(result);
    driver.drain().await;
    lock(&driver.group.state)
        .terminal
        .as_ref()
        .expect("terminal outcome retained")
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_terminal_error_survives_an_outer_cooperative_failure() {
        let group = Shared::new(TaskId(1));
        lock(&group.state).terminal = Some(Err(RuntimeError::user(Box::new(crate::Exited(37)))));
        let error = group
            .resolve(Err(RuntimeError::new("outer loader work failed")))
            .unwrap_err();
        assert_eq!(error.downcast::<crate::Exited>().unwrap().0, 37);
        assert!(lock(&group.state).terminal.is_none());
    }
}
