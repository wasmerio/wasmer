use super::*;
use wasmer::{AsStoreAsync, StoreAsync};

struct Coordinator<S: Syscalls + Control + Send + Sync + 'static> {
    transaction: Option<LoaderTransaction>,
    cleanup: Option<LocalFuture<'static, Result<()>>>,
    root: Arc<Root>,
    local: Arc<Mutex<Local>>,
    env: FunctionEnv<ExecEnv<S>>,
    store: StoreAsync,
    syscalls: Arc<S>,
    host: Arc<dyn wasixv2_host::Host>,
    task: TaskId,
}
impl<S: Syscalls + Control + Send + Sync + 'static> Coordinator<S> {
    async fn abort(mut self) -> Result<()> {
        drop(self.transaction.take());
        self.cleanup = Some(Box::pin(cleanup(
            self.root.clone(),
            self.local.clone(),
            self.store.store(),
            self.syscalls.clone(),
            self.task,
        )));
        let result = await_cleanup(&mut self.cleanup).await;
        if result.is_err() {
            let _ = self.syscalls.loader_fatal(self.root.gate()?);
        }
        result
    }
}
// Keep the pinned operation in its cancellation owner until it completes. In particular,
// a consumed reservation's suspended release future must continue, not be recreated.
async fn await_cleanup(pending: &mut Option<LocalFuture<'static, Result<()>>>) -> Result<()> {
    let result = pending
        .as_mut()
        .ok_or_else(|| failure("missing cleanup"))?
        .await;
    *pending = None;
    result
}
impl<S: Syscalls + Control + Send + Sync + 'static> Drop for Coordinator<S> {
    fn drop(&mut self) {
        let active = self.transaction.is_some();
        drop(self.transaction.take());
        let pending = self.cleanup.take().or_else(|| {
            active.then(|| {
                Box::pin(cleanup(
                    self.root.clone(),
                    self.local.clone(),
                    self.store.store(),
                    self.syscalls.clone(),
                    self.task,
                )) as LocalFuture<'static, Result<()>>
            })
        });
        if let Some(pending) = pending {
            let root = self.root.clone();
            let syscalls = self.syscalls.clone();
            let env = self.env.clone();
            self.host.executor().spawn_local(Box::pin(async move {
                let _keep_env = env;
                if let Err(error) = pending.await {
                    let _ = syscalls.loader_fatal(root.gate().unwrap());
                    tracing::error!(mm=root.gate().map(|gate|gate.mm()).unwrap_or(0),%error,"loader cleanup failed");
                }
            }));
        }
    }
}
async fn cleanup<S: Syscalls + Control + Send + Sync + 'static>(
    root: Arc<Root>,
    local: Arc<Mutex<Local>>,
    store: StoreAsync,
    syscalls: Arc<S>,
    task: TaskId,
) -> Result<()> {
    let gate = root.gate()?.clone();
    let work = lock(&root.state).work.clone();
    let committed = work
        .as_ref()
        .is_some_and(|work| work.committed.load(Ordering::Acquire));
    let stage = {
        let mut store = store.write_lock().await;
        let mut local = lock(&local);
        if committed {
            if let Some(mut stage) = local.stage.take() {
                local.leases.extend(std::mem::take(&mut stage.leases));
                for module in stage.modules.into_values() {
                    local.instances.push(module.publish()?);
                }
                local.tls = stage.tls;
                local.leases.extend(stage.leases);
            }
            None
        } else {
            if let Some(stage) = &mut local.stage {
                for module in stage.modules.values_mut() {
                    module.rollback(&mut store)?;
                }
            }
            local.stage.take()
        }
    };
    if let Some(stage) = stage {
        for lease in stage.leases {
            lease.release().await;
        }
    }
    if let Some(command) = gate.command(task) {
        gate.acknowledge(task, command).map_err(failure)?;
    }
    if let Some(transaction) = gate.recover(task) {
        transaction.wait_ready().await;
        if let Some(work) = work {
            let leases = std::mem::take(&mut *lock(&work.leases));
            if committed {
                lock(&root.state).leases.extend(leases);
            } else {
                for lease in leases {
                    lease.release().await;
                }
            }
        }
        lock(&root.state).work = None;
        transaction.finish().map_err(failure)?;
    } else {
        let _ = syscalls.loader_sleep(task, &gate, LoaderWait::Idle).await;
    }
    Ok(())
}
impl<S: Syscalls + Control + Send + Sync + 'static> StoreImage<S> {
    pub(super) async fn service(&self, env: &AsyncFunctionEnvMut<ExecEnv<S>>) -> Result<()> {
        let (syscalls, task) = {
            let data = env.read().await;
            (data.data().syscalls().clone(), data.data().task())
        };
        let gate = self.root.gate()?.clone();
        // exec/exit can detach this old store before its final complete(). Never wait
        // on old-mm work or allocate against the same task id's replacement image.
        if !syscalls
            .loader_gate(task)
            .is_some_and(|current| Arc::ptr_eq(&current, &gate))
        {
            return Ok(());
        }
        while gate.busy() {
            if !gate.participants().contains(&task) {
                return Ok(());
            }
            if let Some(command) = gate.command(task) {
                self.command(env, task, command).await?;
            }
            if gate.busy() {
                syscalls
                    .loader_sleep(
                        task,
                        &gate,
                        LoaderWait::Command {
                            task,
                            previous: None,
                        },
                    )
                    .await
                    .map_err(failure)?;
            }
        }
        Ok(())
    }
    async fn finish_phase(
        &self,
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        coordinator: &Coordinator<S>,
    ) -> Result<()> {
        let transaction = coordinator.transaction.as_ref().unwrap();
        let gate = transaction.gate();
        if let Some(command) = gate.command(coordinator.task) {
            self.command(env, coordinator.task, command).await?;
        }
        coordinator
            .syscalls
            .loader_sleep(
                coordinator.task,
                gate,
                LoaderWait::Acknowledged {
                    epoch: transaction.epoch(),
                },
            )
            .await
            .map_err(failure)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn open(
        &self,
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &Caller<'_>,
        file: Cstr,
        flags: DlFlags,
        search: Cstr,
        caller_addr: u32,
        handle: DlHandlePtr,
        err: UserSliceMut,
    ) -> SysResult<i32> {
        if caller_addr != 0
            || flags.bits()
                & !(DlFlags::RTLD_NOW.bits()
                    | DlFlags::RTLD_GLOBAL.bits()
                    | DlFlags::RTLD_NOLOAD.bits()
                    | DlFlags::RTLD_NODELETE.bits())
                != 0
            || !flags.contains(DlFlags::RTLD_NOW)
        {
            return report(
                caller,
                err,
                "unsupported loader mode/caller",
                Errno::EOPNOTSUPP,
            )
            .await;
        }
        let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?.clone();
        let current_gate = env
            .read()
            .await
            .data()
            .syscalls()
            .loader_gate(caller.task());
        if !current_gate.is_some_and(|current| Arc::ptr_eq(&current, &gate)) {
            return Err(Errno::ESTALE.into());
        }
        if gate.busy() {
            self.service(env).await.map_err(|_| Errno::EINTR)?;
            return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
        }
        let name = if file.addr() == 0 {
            Vec::new()
        } else {
            read_cstr(caller.memory(), file, 4096)
                .await
                .map_err(|_| Errno::EFAULT)?
        };
        let search = read_cstr(caller.memory(), search, 65536)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let old = self.root.graph();
        let (mut graph, requested) = match collect(env, caller, &old, &name, &search, flags).await {
            Ok(value) => value,
            Err(error) => {
                if !env
                    .read()
                    .await
                    .data()
                    .syscalls()
                    .loader_gate(caller.task())
                    .is_some_and(|current| Arc::ptr_eq(&current, &gate))
                {
                    return Err(Errno::ESTALE.into());
                }
                return report(caller, err, &error.to_string(), Errno::ENOEXEC).await;
            }
        };
        let closing = match gate.close(caller.task()) {
            Ok(closing) => closing,
            Err(Errno::EAGAIN) => return Err(wasixv2_syscalls::SysErr::RestartNoIntr),
            Err(error) => return Err(error.into()),
        };
        let syscalls = env.read().await.data().syscalls().clone();
        syscalls
            .loader_sleep(caller.task(), &gate, LoaderWait::Drained)
            .await?;
        if !Arc::ptr_eq(&old, &self.root.graph()) {
            return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
        }
        let transaction = syscalls.loader_activate(caller.task(), closing)?;
        let (host, store) = {
            let data = env.read().await;
            (data.data().host().clone(), env.as_store_async().store())
        };
        let mut coordinator = Coordinator {
            transaction: Some(transaction),
            cleanup: None,
            root: self.root.clone(),
            local: self.local.clone(),
            env: env.as_ref(),
            store,
            syscalls: syscalls.clone(),
            host,
            task: caller.task(),
        };
        let work = Arc::new(Work {
            publication: Mutex::new(None),
            previous: old.libraries.len(),
            first: caller.task(),
            leases: Mutex::new(Vec::new()),
            committed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        lock(&self.root.state).work = Some(work.clone());
        let result = async {
            self.finish_phase(env, &coordinator).await?;
            for lib in &mut graph.libraries[work.previous..] {
                let info = lib.metadata.memory_info();
                let lease = syscalls
                    .loader_reserve(caller, info.memory_size, 1u32 << info.memory_alignment)
                    .await
                    .map_err(failure)?;
                lib.base = lease.address();
                lock(&work.leases).push(lease);
            }
            let graph = Arc::new(graph);
            let mut references = lock(&self.root.state).references.clone();
            let count = references.entry(requested).or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| failure("loader reference overflow"))?;
            let mut ctors = lock(&self.root.state).ctors.clone();
            for &index in &graph.initialization {
                if index >= work.previous {
                    let slot = if let Some(export) = graph.libraries[index]
                        .metadata
                        .exports()
                        .get("__wasm_call_ctors")
                    {
                        *graph.libraries[index]
                            .slots
                            .get(&export.index)
                            .ok_or_else(|| failure("constructor has no slot"))?
                    } else {
                        0
                    };
                    ctors.push_back(slot);
                }
            }
            *lock(&work.publication) = Some(Publication {
                graph,
                references,
                ctors,
            });
            for phase in [LoaderPhase::Prepare, LoaderPhase::Apply] {
                coordinator
                    .transaction
                    .as_ref()
                    .unwrap()
                    .advance(phase)
                    .map_err(failure)?;
                self.finish_phase(env, &coordinator).await?;
                if let Some(error) = lock(&work.error).clone() {
                    return Err(failure(error));
                }
            }
            // User copy is the last recoverable operation. No guest runs while it completes;
            // all following state swaps/phase transition have been prepared and cannot allocate.
            let publication = lock(&work.publication)
                .take()
                .ok_or_else(|| failure("missing publication"))?;
            lock(&self.root.state)
                .leases
                .try_reserve(lock(&work.leases).len())
                .map_err(failure)?;
            write_value(caller.memory(), handle, &DlHandle(requested as u32 + 1))
                .await
                .map_err(failure)?;
            {
                let mut state = lock(&self.root.state);
                state.graph = publication.graph;
                state.references = publication.references;
                state.ctors = publication.ctors;
                work.committed.store(true, Ordering::Release);
            }
            if let Err(error) = coordinator
                .transaction
                .as_ref()
                .unwrap()
                .advance(LoaderPhase::Publish)
            {
                let _ = syscalls.loader_fatal(&gate);
                return Err(failure(error));
            }
            self.finish_phase(env, &coordinator).await?;
            lock(&self.root.state)
                .leases
                .extend(std::mem::take(&mut *lock(&work.leases)));
            lock(&self.root.state).work = None;
            coordinator
                .transaction
                .take()
                .unwrap()
                .finish()
                .map_err(failure)?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => Ok(0),
            Err(error) => {
                let committed = work.committed.load(Ordering::Acquire);
                if committed {
                    let _ = syscalls.loader_fatal(&gate);
                    drop(coordinator);
                    return Err(Errno::EIO.into());
                }
                coordinator.abort().await.map_err(|_| Errno::EIO)?;
                report(caller, err, &error.to_string(), Errno::ENOEXEC).await
            }
        }
    }
}
pub(super) async fn report(
    caller: &Caller<'_>,
    buffer: UserSliceMut,
    message: &str,
    errno: Errno,
) -> SysResult<i32> {
    let mut bytes = message.as_bytes().to_vec();
    bytes.truncate(buffer.len.saturating_sub(1) as usize);
    if buffer.len > 0 {
        bytes.push(0);
        buffer
            .copy_out(caller.memory(), &bytes)
            .await
            .map_err(|_| Errno::EFAULT)?;
    }
    Ok(-errno.0)
}
async fn collect<S: Syscalls + Control + Send + Sync + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    caller: &Caller<'_>,
    old: &Graph,
    name: &[u8],
    search: &[u8],
    flags: DlFlags,
) -> Result<(Graph, usize)> {
    let syscalls = env.read().await.data().syscalls().clone();
    let mut graph = old.clone();
    if name.is_empty() {
        return Ok((graph, 0));
    }
    async fn read<S: Control>(
        syscalls: &S,
        caller: &Caller<'_>,
        name: &[u8],
        search: &[u8],
        runtime: &[String],
    ) -> Result<wasixv2_syscalls::LibraryImage> {
        let paths = if name.contains(&b'/') {
            vec![name.to_vec()]
        } else {
            let mut paths = Vec::new();
            for path in runtime
                .iter()
                .flat_map(|p| p.as_bytes().split(|b| *b == b':'))
                .chain(search.split(|b| *b == b':'))
            {
                if path.contains(&b'$') {
                    return Err(failure("runtime path expansion is unsupported"));
                }
                let mut candidate = path.to_vec();
                if !candidate.is_empty() {
                    candidate.push(b'/');
                }
                candidate.extend_from_slice(name);
                paths.push(candidate);
            }
            paths
        };
        let mut last = None;
        for path in paths {
            match syscalls.loader_read(caller, &path).await {
                Ok(image) => return Ok(image),
                Err(error) => last = Some(error),
            }
        }
        Err(failure(format!("cannot open library: {last:?}")))
    }
    let image = read(
        &*syscalls,
        caller,
        name,
        search,
        old.libraries[0].metadata.runtime_path(),
    )
    .await?;
    let requested = if let Some(index) = graph
        .libraries
        .iter()
        .position(|lib| lib.identity == Some(image.identity))
    {
        index
    } else {
        if flags.contains(DlFlags::RTLD_NOLOAD) {
            return Err(failure("RTLD_NOLOAD library is not loaded"));
        }
        let mut library = Library::new(image.image.bytes().to_vec(), Some(image.identity))?;
        library.backing = Some(image.image.clone());
        graph.libraries.push(library);
        graph.libraries.len() - 1
    };
    let mut queue = VecDeque::from([requested]);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(index) = queue.pop_front() {
        if !seen.insert(index) || index < old.libraries.len() {
            continue;
        }
        let needed = graph.libraries[index].metadata.needed().to_vec();
        let runtime = graph.libraries[index].metadata.runtime_path().to_vec();
        for name in needed {
            let image = read(&*syscalls, caller, name.as_bytes(), search, &runtime).await?;
            let dependency = if let Some(index) = graph
                .libraries
                .iter()
                .position(|lib| lib.identity == Some(image.identity))
            {
                index
            } else {
                let mut library = Library::new(image.image.bytes().to_vec(), Some(image.identity))?;
                library.backing = Some(image.image.clone());
                graph.libraries.push(library);
                graph.libraries.len() - 1
            };
            graph.libraries[index].dependencies.push(dependency);
            queue.push_back(dependency);
        }
    }
    if flags.contains(DlFlags::RTLD_GLOBAL) {
        for index in graph.scope(requested) {
            if !graph.global.contains(&index) {
                graph.global.push(index);
            }
        }
    }
    for index in old.libraries.len()..graph.libraries.len() {
        let info = graph.libraries[index].metadata.memory_info();
        graph.cursor = align(graph.cursor, 1u32 << info.table_alignment)?;
        graph.libraries[index].table = graph.cursor;
        graph.cursor = graph
            .cursor
            .checked_add(info.table_size)
            .ok_or_else(|| failure("table overflow"))?;
        graph.assign_slots(index)?;
    }
    graph.seal(requested, old.libraries.len())?;
    Ok((graph, requested))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        sync::atomic::AtomicUsize,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn canceled_abort_keeps_the_same_suspended_release_for_drop_transfer() {
        let started = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let mut pending: Option<LocalFuture<'static, Result<()>>> = Some(Box::pin({
            let (started, ready, completed) = (started.clone(), ready.clone(), completed.clone());
            async move {
                started.fetch_add(1, Ordering::Relaxed);
                std::future::poll_fn(|_| {
                    if ready.load(Ordering::Acquire) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                completed.store(true, Ordering::Release);
                Ok(())
            }
        }));
        let mut cx = Context::from_waker(Waker::noop());
        let mut abort = Box::pin(await_cleanup(&mut pending));
        assert!(abort.as_mut().poll(&mut cx).is_pending());
        drop(abort);
        assert_eq!(started.load(Ordering::Relaxed), 1);
        assert!(!completed.load(Ordering::Acquire));
        // This is the exact future Coordinator::drop passes to the existing executor.
        let mut transferred = pending.take().expect("cancellation retained cleanup");
        ready.store(true, Ordering::Release);
        assert!(matches!(
            transferred.as_mut().poll(&mut cx),
            Poll::Ready(Ok(()))
        ));
        assert!(completed.load(Ordering::Acquire));
        assert_eq!(started.load(Ordering::Relaxed), 1);
    }
}
