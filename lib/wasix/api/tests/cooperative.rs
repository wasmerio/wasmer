//! Real syscall-entry/exit ptrace stops around the store-local host-work driver.
#![cfg(all(feature = "sys", feature = "tokio", not(target_arch = "wasm32")))]

use std::{
    any::Any,
    future::Future,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};
use wasixv2_abi::{
    GuestPtr, GuestPtrMut, Idtype, PtraceOptions, PtraceRequest, RusagePtr, Siginfo, Signal,
    WaitOptions,
};
use wasixv2_exec::{
    Abi, AbiInstance, Abis, Entry, EntryReturn, ExecEnv, ExecError, Role, Sigreturn, VforkResume,
    Wasixv2,
};
use wasixv2_host::{ByteSink, EmptySource, Host, LocalFuture, TokioHost};
use wasixv2_kernel::{Dispatch, HostClock, Kernel, ProcessConfig, Stdio, SyscallEntry};
use wasixv2_syscalls::{
    Caller, Control, Fault, GroupStop, GuestAction, Layer, Layered, LoaderControl, LoaderPhase,
    LoaderWait, MaybeFuture, SignalFrame, SysResult, Syscall, SyscallRet, Syscalls, TaskId,
    UserMemory, read_value,
};
use wasixv2_vfs_mem::MemFs;
use wasmer::{
    AsyncFunctionEnvMut, FunctionEnv, Global, Imports, Instance, Module, RuntimeError, Store, Value,
};

struct CountCalls(Arc<AtomicUsize>);
impl Layer<Kernel> for CountCalls {
    fn call<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        call: Syscall,
        next: &'a Kernel,
    ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
        if matches!(call, Syscall::Getpid) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        Dispatch.call(caller, call, next)
    }
}
type Stack = Layered<SyscallEntry<CountCalls>, Kernel>;

struct Sink;
impl ByteSink for Sink {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move { Ok(data.len()) })
    }
}
struct Memory(Mutex<Vec<u8>>);
impl UserMemory for Memory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let memory = self.0.lock().unwrap();
            let source = memory
                .get(addr as usize..addr as usize + buf.len())
                .ok_or(Fault)?;
            buf.copy_from_slice(source);
            Ok(())
        })
    }
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        Box::pin(async move {
            let mut memory = self.0.lock().unwrap();
            memory
                .get_mut(addr as usize..addr as usize + data.len())
                .ok_or(Fault)?
                .copy_from_slice(data);
            Ok(())
        })
    }
    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { self.0.lock().unwrap().len() as u64 })
    }
    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
}

struct Probe {
    enabled: bool,
    phases: AtomicUsize,
}
struct TestAbi(Arc<Probe>);
impl Abi<Stack> for TestAbi {
    fn name(&self) -> &'static str {
        "cooperation-proof"
    }
    fn matches(&self, _: &Module) -> bool {
        true
    }
    fn matches_image(&self, _: &[u8]) -> bool {
        true
    }
    fn imports(
        &self,
        store: &mut Store,
        env: &FunctionEnv<ExecEnv<Stack>>,
        module: &Module,
    ) -> Imports {
        <Wasixv2 as Abi<Stack>>::imports(&Wasixv2, store, env, module)
    }
    fn bind(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> Result<Arc<dyn AbiInstance<Stack>>, ExecError> {
        Ok(Arc::new(TestInstance {
            inner: <Wasixv2 as Abi<Stack>>::bind(&Wasixv2, instance, store, task, role)?,
            host_progress: instance.exports.get_global("host_progress")?.clone(),
            probe: self.0.clone(),
        }))
    }
}
struct TestInstance {
    inner: Arc<dyn AbiInstance<Stack>>,
    host_progress: Global,
    probe: Arc<Probe>,
}
impl AbiInstance<Stack> for TestInstance {
    fn cooperative_loader(&self) -> bool {
        self.probe.enabled
    }
    fn loader_work_outside<'a>(
        &'a self,
        entry: &'a Entry<'a, Stack>,
    ) -> LocalFuture<'a, Result<(), RuntimeError>> {
        Box::pin(async move {
            let (syscalls, task) = entry
                .with_store(|env, _| (env.syscalls().clone(), env.task()))
                .await;
            let gate = syscalls.loader_gate(task).unwrap();
            while gate.busy() {
                if let Some(command) = gate.command(task) {
                    let value = self.probe.phases.fetch_add(1, Ordering::Relaxed) + 1;
                    // The only work is a known i32 global assignment under the store lock.
                    // No wasm helper, handler, constructor or syscall future is invoked.
                    entry
                        .with_store(|_, store| {
                            self.host_progress.set(store, Value::I32(value as i32))
                        })
                        .await?;
                    gate.acknowledge(task, command).unwrap();
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
                        .map_err(|error| RuntimeError::new(format!("{error:?}")))?;
                }
            }
            Ok(())
        })
    }
    fn start<'a>(
        &'a self,
        entry: &'a Entry<'a, Stack>,
    ) -> LocalFuture<'a, Result<(), RuntimeError>> {
        self.inner.start(entry)
    }
    fn on_entry_return(&self) -> EntryReturn {
        self.inner.on_entry_return()
    }
    fn start_thread<'a>(
        &'a self,
        entry: &'a Entry<'a, Stack>,
        tid: i32,
        arg: u32,
    ) -> LocalFuture<'a, Result<(), RuntimeError>> {
        self.inner.start_thread(entry, tid, arg)
    }
    fn on_thread_return(&self) -> EntryReturn {
        self.inner.on_thread_return()
    }
    fn deliver_signal<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<Stack>>,
        frame: &'a SignalFrame,
        restart: bool,
    ) -> LocalFuture<'a, Result<Sigreturn, RuntimeError>> {
        self.inner.deliver_signal(env, frame, restart)
    }
    fn vfork_resume<'a>(
        &'a self,
        env: &'a AsyncFunctionEnvMut<ExecEnv<Stack>>,
        child: TaskId,
        pid: i32,
    ) -> LocalFuture<'a, VforkResume> {
        self.inner.vfork_resume(env, child, pid)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

async fn ptrace(
    kernel: &Kernel,
    caller: &Caller<'_>,
    request: PtraceRequest,
    child: TaskId,
    addr: u32,
    data: u32,
) -> i32 {
    kernel
        .ptrace(caller, request, child.0 as i32, addr, data)
        .await
        .unwrap_or_else(|error| panic!("ptrace {request:?}: {error:?}"))
}
async fn wait(kernel: &Kernel, caller: &Caller<'_>, child: TaskId) -> Option<i32> {
    kernel
        .waitid(
            caller,
            Idtype::P_PID,
            child.0 as i32,
            GuestPtrMut::new(0x500),
            WaitOptions(WaitOptions::WNOHANG.0 | WaitOptions::WEXITED.0 | WaitOptions::__WALL.0),
            RusagePtr::NULL,
        )
        .await
        .unwrap();
    let info: Siginfo = read_value(caller.memory(), GuestPtr::new(0x500))
        .await
        .unwrap();
    let pid = i32::from_le_bytes(info.sifields[0..4].try_into().unwrap());
    (pid != 0).then(|| i32::from_le_bytes(info.sifields[8..12].try_into().unwrap()))
}
async fn syscall_info(kernel: &Kernel, caller: &Caller<'_>, child: TaskId) -> Vec<u8> {
    ptrace(
        kernel,
        caller,
        PtraceRequest::PTRACE_GET_SYSCALL_INFO,
        child,
        88,
        0x700,
    )
    .await;
    let mut info = vec![0; 88];
    caller.memory().read(0x700, &mut info).await.unwrap();
    info
}

const GUEST: &str = r#"(module
    (import "wasixv2" "getpid" (func $getpid (result i32)))
    (import "wasixv2" "exit_group" (func $exit (param i32)))
    (memory (export "memory") 1 2 shared)
    (global (export "host_progress") (mut i32) (i32.const 0))
    (func (export "_start")
      (drop (call $getpid))
      (i32.store (i32.const 0x1000) (i32.const 1))
      (call $exit (i32.const 0))))"#;

async fn scenario(enabled: bool) {
    let host: Arc<dyn Host> = Arc::new(TokioHost::default());
    let fs = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
    let kernel = Kernel::new(host.clone(), Arc::new(fs));
    let parent = kernel
        .spawn(ProcessConfig {
            args: vec!["tracer".into()],
            env: vec![],
            stdio: Stdio {
                stdin: Arc::new(EmptySource),
                stdout: Arc::new(Sink),
                stderr: Arc::new(Sink),
            },
        })
        .unwrap();
    let memory = Memory(Mutex::new(vec![0; 65536]));
    let caller = Caller::new(parent, &memory);
    // The real CLONE_VM child is a separate ptraceable process group. Its queued
    // StartThread is consumed because this test explicitly owns its guest runner.
    let child = TaskId(
        kernel
            .thread_spawn(&caller, 0x111, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL)
            .await
            .unwrap() as u32,
    );
    assert!(
        matches!(kernel.take_pending(parent), Some(GuestAction::StartThread { task, .. }) if task == child)
    );
    let child_caller = Caller::new(child, &memory);
    ptrace(
        &kernel,
        &child_caller,
        PtraceRequest::PTRACE_TRACEME,
        TaskId(0),
        0,
        0,
    )
    .await;
    kernel.kill(child, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child), Some(GuestAction::Stopped));
    assert_eq!(wait(&kernel, &caller, child).await, Some(Signal::SIGSTOP.0));
    ptrace(
        &kernel,
        &caller,
        PtraceRequest::PTRACE_SETOPTIONS,
        child,
        0,
        PtraceOptions::PTRACE_O_TRACESYSGOOD.0,
    )
    .await;
    ptrace(&kernel, &caller, PtraceRequest::PTRACE_SYSCALL, child, 0, 0).await;
    let gate = kernel.loader_gate(child).unwrap();
    drop(gate.bind(parent).unwrap());
    let bodies = Arc::new(AtomicUsize::new(0));
    let stack = Arc::new(Layered::new(
        SyscallEntry(CountCalls(bodies.clone())),
        kernel,
    ));
    let kernel = &stack.inner;
    let probe = Arc::new(Probe {
        enabled,
        phases: AtomicUsize::new(0),
    });
    let mut abis = Abis::new();
    abis.push(Arc::new(TestAbi(probe.clone())));
    let store = Store::default();
    let module = wasixv2_exec::load(store.engine(), &wat::parse_str(GUEST).unwrap()).unwrap();
    let mut run = Box::pin(wasixv2_exec::run_with_abis(
        store,
        &module,
        stack.clone(),
        child,
        host,
        Arc::new(abis),
    ));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(run.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        wait(kernel, &caller, child).await,
        Some(Signal::SIGTRAP.0 | 0x80)
    );
    let entry = syscall_info(kernel, &caller, child).await;
    assert_eq!(entry[0], 1);
    assert_eq!(u64::from_le_bytes(entry[24..32].try_into().unwrap()), 172);
    assert_eq!(bodies.load(Ordering::Relaxed), 0);
    let transaction = kernel
        .loader_activate(parent, gate.close(parent).unwrap())
        .unwrap();
    gate.acknowledge(parent, gate.command(parent).unwrap())
        .unwrap();
    assert!(run.as_mut().poll(&mut cx).is_pending());
    if enabled {
        for phase in [
            LoaderPhase::Prepare,
            LoaderPhase::Apply,
            LoaderPhase::Publish,
        ] {
            transaction.advance(phase).unwrap();
            gate.acknowledge(parent, gate.command(parent).unwrap())
                .unwrap();
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        transaction.finish().unwrap();
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(probe.phases.load(Ordering::Relaxed), 4);
        assert_eq!(syscall_info(kernel, &caller, child).await, entry);
        assert_eq!(
            wait(kernel, &caller, child).await,
            None,
            "no replayed ptrace event"
        );
        assert_eq!(bodies.load(Ordering::Relaxed), 0);
        let mut stopped = Box::pin(kernel.stop_wait(child));
        assert!(stopped.as_mut().poll(&mut cx).is_pending());
        drop(stopped);
        ptrace(
            kernel,
            &caller,
            PtraceRequest::PTRACE_PEEKDATA,
            child,
            0x1000,
            0x600,
        )
        .await;
        assert_eq!(
            read_value::<u32>(caller.memory(), GuestPtr::new(0x600))
                .await
                .unwrap(),
            0,
            "no guest instructions resumed"
        );
        ptrace(kernel, &caller, PtraceRequest::PTRACE_SYSCALL, child, 0, 0).await;
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            wait(kernel, &caller, child).await,
            Some(Signal::SIGTRAP.0 | 0x80)
        );
        let exit = syscall_info(kernel, &caller, child).await;
        assert_eq!(exit[0], 2);
        assert_eq!(
            i64::from_le_bytes(exit[24..32].try_into().unwrap()),
            i64::from(child.0)
        );
        assert_eq!(
            bodies.load(Ordering::Relaxed),
            1,
            "the original body ran once"
        );
        // The exit stop owns the same NoInterrupt syscall future. A second load
        // must make progress here too without delivering that result to wasm.
        let exit_round = kernel
            .loader_activate(parent, gate.close(parent).unwrap())
            .unwrap();
        gate.acknowledge(parent, gate.command(parent).unwrap())
            .unwrap();
        assert!(run.as_mut().poll(&mut cx).is_pending());
        for phase in [
            LoaderPhase::Prepare,
            LoaderPhase::Apply,
            LoaderPhase::Publish,
        ] {
            exit_round.advance(phase).unwrap();
            gate.acknowledge(parent, gate.command(parent).unwrap())
                .unwrap();
            assert!(run.as_mut().poll(&mut cx).is_pending());
        }
        exit_round.finish().unwrap();
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(probe.phases.load(Ordering::Relaxed), 8);
        assert_eq!(syscall_info(kernel, &caller, child).await, exit);
        assert_eq!(wait(kernel, &caller, child).await, None);
        assert_eq!(bodies.load(Ordering::Relaxed), 1);
        ptrace(
            kernel,
            &caller,
            PtraceRequest::PTRACE_PEEKDATA,
            child,
            0x1000,
            0x600,
        )
        .await;
        assert_eq!(
            read_value::<u32>(caller.memory(), GuestPtr::new(0x600))
                .await
                .unwrap(),
            0
        );
        ptrace(kernel, &caller, PtraceRequest::PTRACE_CONT, child, 0, 0).await;
        assert!(matches!(run.as_mut().poll(&mut cx), Poll::Ready(Ok(0))));
    } else {
        assert_eq!(probe.phases.load(Ordering::Relaxed), 0);
        assert!(
            transaction.advance(LoaderPhase::Prepare).is_err(),
            "opt-out never fabricates acknowledgement"
        );
        ptrace(kernel, &caller, PtraceRequest::PTRACE_CONT, child, 0, 0).await;
        assert!(matches!(run.as_mut().poll(&mut cx), Poll::Ready(Ok(0))));
        for phase in [
            LoaderPhase::Prepare,
            LoaderPhase::Apply,
            LoaderPhase::Publish,
        ] {
            transaction.advance(phase).unwrap();
            gate.acknowledge(parent, gate.command(parent).unwrap())
                .unwrap();
        }
        transaction.finish().unwrap();
        assert_eq!(probe.phases.load(Ordering::Relaxed), 0);
    }
    assert_eq!(bodies.load(Ordering::Relaxed), 1);
    gate.leave(parent);
}

#[tokio::test]
async fn real_ptrace_pending_syscall_survives_cooperative_publication() {
    tokio::task::LocalSet::new().run_until(scenario(true)).await;
}

#[tokio::test]
async fn custom_abi_opt_out_keeps_the_existing_ptrace_wait() {
    tokio::task::LocalSet::new()
        .run_until(scenario(false))
        .await;
}
