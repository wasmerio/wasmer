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
        let pending = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.contexts.clear();
            std::mem::take(&mut state.pending)
        };
        drop(pending);
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
        Ok(())
    }
}

/// Capture before returning a host-import future: outside the native coroutine
/// the exact retained-borrow boundary is no longer installed in TLS.
pub fn context_operations_supported() -> bool {
    #[cfg(feature = "sys")]
    {
        wasmer::sys::context_switch_supported()
    }
    #[cfg(not(feature = "sys"))]
    {
        false
    }
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
}
impl<S: Send + Sync + 'static> Driver<S> {
    async fn activate(&mut self, id: u64) {
        lock(&self.group.state).current = id;
        if self.installed == id {
            return;
        }
        let roots = &mut self.roots;
        let previous = self.installed;
        crate::asyncify::Outside {
            env: &self.env,
            store: &self.store,
        }
        .with(|env, _| {
            if let Some(asyncify) = &mut env.asyncify {
                let old = asyncify.take_context();
                if let Some(root) = roots.get_mut(&previous) {
                    root.asyncify = old;
                }
                asyncify.put_context(std::mem::take(&mut roots.get_mut(&id).unwrap().asyncify));
            }
        })
        .await;
        self.installed = id;
    }

    async fn retire(&mut self, id: u64) {
        self.activate(id).await;
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
    }
}
impl<S: Send + Sync + 'static> Drop for Driver<S> {
    fn drop(&mut self) {
        self.group.close();
        if self.roots.is_empty() {
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
        };
        self.cleanup.spawn(Box::pin(async move {
            pending.drain().await;
        }));
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
            #[cfg(feature = "sys")]
            {
                store.engine().is_sys() && env.contexts_supported
            }
            #[cfg(not(feature = "sys"))]
            {
                let _ = (env, store);
                false
            }
        })
        .await;
    if !supported {
        return guest.await;
    }
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
    attachments
        .as_mut()
        .expect("contexts require invocation cleanup custody")
        .retain_contexts(group.clone());
    let mut driver = Driver {
        group,
        roots: BTreeMap::from([(
            0,
            Invocation {
                future: Some(Box::pin(guest)),
                cpu: cpu.clone(),
                asyncify: Default::default(),
                system: false,
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
            let root = driver
                .roots
                .get_mut(&active)
                .expect("active context has a root");
            let polled = driver.cpu.run(&mut root.system, || {
                root.future.as_mut().unwrap().as_mut().poll(cx)
            });
            if let Poll::Ready(result) = polled {
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
