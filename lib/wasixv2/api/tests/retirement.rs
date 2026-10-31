//! Selected native image metadata may outlive its managed guest runner.
#![cfg(all(feature = "sys", feature = "tokio", not(target_arch = "wasm32")))]

use std::{
    io,
    sync::{Arc, Mutex},
};
use wasixv2_exec::{Abi, AbiInstance, Abis, ExecEnv, ExecError, Role};
use wasixv2_host::{ByteSink, EmptySource, Host, LocalFuture, TokioHost};
use wasixv2_kernel::{Dispatch, HostClock, Kernel, ProcessConfig, Stdio, SyscallEntry};
use wasixv2_syscalls::{
    Caller, Errno, Layer, Layered, LoaderControl, LoaderGate, MaybeFuture, SysResult, Syscall,
    SyscallRet, TaskId,
};
use wasixv2_vfs_mem::MemFs;
use wasmer::{FunctionEnv, Imports, Instance, Module, Store};

type ExitGate = Arc<Mutex<Option<Arc<LoaderGate>>>>;
#[derive(Default)]
struct RecordExit(ExitGate);
impl Layer<Kernel> for RecordExit {
    fn call<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        call: Syscall,
        next: &'a Kernel,
    ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
        if matches!(call, Syscall::ExitGroup { .. }) {
            *self.0.lock().unwrap() = next.loader_gate(caller.task());
        }
        Dispatch.call(caller, call, next)
    }
}
type Stack = Layered<SyscallEntry<RecordExit>, Kernel>;
type Selected = Arc<Mutex<Vec<Arc<dyn Abi<Stack>>>>>;

struct CaptureNative(Selected);
impl Abi<Stack> for CaptureNative {
    fn name(&self) -> &'static str {
        "retained-native-proof"
    }
    fn matches(&self, module: &Module) -> bool {
        <wasixv2_dylink::Native as Abi<Stack>>::matches(&wasixv2_dylink::Native, module)
    }
    fn matches_image(&self, image: &[u8]) -> bool {
        <wasixv2_dylink::Native as Abi<Stack>>::matches_image(&wasixv2_dylink::Native, image)
    }
    fn for_image(&self, module: &Module) -> Option<Arc<dyn Abi<Stack>>> {
        let selected =
            <wasixv2_dylink::Native as Abi<Stack>>::for_image(&wasixv2_dylink::Native, module)?;
        self.0.lock().unwrap().push(selected.clone());
        Some(selected)
    }
    fn imports(
        &self,
        store: &mut Store,
        env: &FunctionEnv<ExecEnv<Stack>>,
        module: &Module,
    ) -> Imports {
        <wasixv2_dylink::Native as Abi<Stack>>::imports(&wasixv2_dylink::Native, store, env, module)
    }
    fn bind(
        &self,
        instance: &Instance,
        store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> Result<Arc<dyn AbiInstance<Stack>>, ExecError> {
        <wasixv2_dylink::Native as Abi<Stack>>::bind(
            &wasixv2_dylink::Native,
            instance,
            store,
            task,
            role,
        )
    }
}
struct Sink;
impl ByteSink for Sink {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move { Ok(data.len()) })
    }
}

enum End {
    Normal,
    Fault,
    ImportFailure,
}
fn source(end: &End) -> Vec<u8> {
    let import = if matches!(end, End::ImportFailure) {
        "(import \"env\" \"unsupported_main_global\" (global i32))"
    } else {
        ""
    };
    let body = if matches!(end, End::Fault) {
        "unreachable"
    } else {
        "(call $exit (i32.const 0))"
    };
    pic(import, "", body)
}

fn pic(imports: &str, data: &str, body: &str) -> Vec<u8> {
    let mut bytes = wat::parse_str(format!(
        r#"(module
        (import "env" "memory" (memory 1 65536 shared))
        (import "env" "table" (table 3 funcref))
        (import "env" "__stack_pointer" (global (mut i32)))
        (import "env" "__memory_base" (global $base i32))
        (import "env" "__table_base" (global i32))
        (import "wasixv2" "exit_group" (func $exit (param i32)))
        {imports}
        {data}
        (func (export "_start") {body}))"#
    ))
    .unwrap();
    // Same source-only custom-section sibling as dylink::tests::bytes. No compiled
    // fixture is checked in: memory16/alignment1, no initial table/data effects.
    let payload = [
        8, b'd', b'y', b'l', b'i', b'n', b'k', b'.', b'0', 1, 4, 16, 0, 0, 0,
    ];
    bytes.extend_from_slice(&[0, payload.len() as u8]);
    bytes.extend_from_slice(&payload);
    bytes
}

async fn retained_image(end: End) {
    let host: Arc<dyn Host> = Arc::new(TokioHost::default());
    let fs = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
    let kernel = Kernel::new(host.clone(), Arc::new(fs));
    let task = kernel
        .spawn(ProcessConfig {
            args: vec!["retained-image".into()],
            env: vec![],
            stdio: Stdio {
                stdin: Arc::new(EmptySource),
                stdout: Arc::new(Sink),
                stderr: Arc::new(Sink),
            },
        })
        .unwrap();
    let old_gate = kernel.loader_gate(task).unwrap();
    let selected: Selected = Arc::default();
    let mut abis = Abis::new();
    abis.push(Arc::new(CaptureNative(selected.clone())));
    let stack = Arc::new(Layered::new(SyscallEntry::default(), kernel));
    let store = Store::default();
    let module = wasixv2_dylink::load(store.engine(), &source(&end)).unwrap();
    let result =
        wasixv2_exec::run_with_abis(store, &module, stack, task, host, Arc::new(abis)).await;
    match end {
        End::Normal => assert_eq!(result.unwrap(), 0),
        End::Fault => assert_eq!(result.unwrap(), 132),
        End::ImportFailure => assert!(result.is_err()),
    }
    let retained = selected
        .lock()
        .unwrap()
        .pop()
        .expect("selected native image");
    assert!(
        matches!(retained.claim_image(&old_gate), Err(Errno::ESTALE)),
        "logical retirement must reject reuse even while metadata is retained"
    );
    let competing = Arc::new(());
    assert!(
        matches!(old_gate.claim_image(&competing), Err(Errno::EBUSY)),
        "retained metadata still owns the image nonce"
    );
    drop(retained);
    assert!(
        old_gate.claim_image(&competing).is_ok(),
        "the final attachment releases ownership"
    );
}

#[tokio::test]
async fn retained_native_metadata_is_retired_after_normal_exit() {
    tokio::task::LocalSet::new()
        .run_until(retained_image(End::Normal))
        .await;
}
#[tokio::test]
async fn retained_native_metadata_is_retired_after_guest_fault() {
    tokio::task::LocalSet::new()
        .run_until(retained_image(End::Fault))
        .await;
}
#[tokio::test]
async fn failed_native_import_binding_still_retires_the_selected_image() {
    tokio::task::LocalSet::new()
        .run_until(retained_image(End::ImportFailure))
        .await;
}

#[tokio::test]
async fn native_exec_retires_old_and_new_images_while_old_metadata_is_retained() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let host: Arc<dyn Host> = Arc::new(TokioHost::default());
            let fs = Arc::new(MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1));
            let next = pic("", "", "(call $exit (i32.const 27))");
            wasixv2::populate_directory(
                fs.clone(),
                &[
                    wasixv2::DirectoryEntry::Directory {
                        path: "".into(),
                        mode: 0o755,
                    },
                    wasixv2::DirectoryEntry::File {
                        path: "next".into(),
                        mode: 0o755,
                        data: next,
                    },
                ],
                1,
                1,
            )
            .await
            .unwrap();
            let kernel = Kernel::new(host.clone(), fs);
            let task = kernel
                .spawn(ProcessConfig {
                    args: vec!["before-exec".into()],
                    env: vec![],
                    stdio: Stdio {
                        stdin: Arc::new(EmptySource),
                        stdout: Arc::new(Sink),
                        stderr: Arc::new(Sink),
                    },
                })
                .unwrap();
            let old_gate = kernel.loader_gate(task).unwrap();
            let selected: Selected = Arc::default();
            let mut abis = Abis::new();
            abis.set_loader(Arc::new(wasixv2_dylink::Compile));
            abis.push(Arc::new(CaptureNative(selected.clone())));
            let abis = Arc::new(abis);
            let kernel = kernel.with_binfmt(abis.clone());
            let new_gate: ExitGate = Arc::default();
            let stack = Arc::new(Layered::new(
                SyscallEntry(RecordExit(new_gate.clone())),
                kernel,
            ));
            // commands.rs::MAIN's execve/argv sibling, using PIC-relative storage.
            let first = pic(
                "(import \"wasixv2\" \"execve\" (func $exec (param i32 i32 i32) (result i32)))",
                r#"(data (global.get $base) "/next\00")"#,
                r#"(i32.store (i32.add (global.get $base) (i32.const 8)) (global.get $base))
                (i32.store (i32.add (global.get $base) (i32.const 12)) (i32.const 0))
                (call $exit (i32.sub (i32.const 0) (call $exec
                  (global.get $base) (i32.add (global.get $base) (i32.const 8)) (i32.const 0))))"#,
            );
            let store = Store::default();
            let module = wasixv2_dylink::load(store.engine(), &first).unwrap();
            assert_eq!(
                wasixv2_exec::run_with_abis(store, &module, stack, task, host, abis)
                    .await
                    .unwrap(),
                27
            );
            let new_gate = new_gate.lock().unwrap().take().expect("next image exited");
            assert!(!Arc::ptr_eq(&old_gate, &new_gate));
            let mut images = std::mem::take(&mut *selected.lock().unwrap());
            assert_eq!(images.len(), 2, "exec selected a fresh native image");
            let current = images.pop().unwrap();
            let previous = images.pop().unwrap();
            assert!(matches!(
                previous.claim_image(&old_gate),
                Err(Errno::ESTALE)
            ));
            assert!(matches!(current.claim_image(&new_gate), Err(Errno::ESTALE)));
            let competing = Arc::new(());
            assert!(matches!(
                old_gate.claim_image(&competing),
                Err(Errno::EBUSY)
            ));
            assert!(matches!(
                new_gate.claim_image(&competing),
                Err(Errno::EBUSY)
            ));
            drop(previous);
            assert!(old_gate.claim_image(&competing).is_ok());
            assert!(matches!(
                new_gate.claim_image(&competing),
                Err(Errno::EBUSY)
            ));
            drop(current);
            assert!(new_gate.claim_image(&competing).is_ok());
        })
        .await;
}
