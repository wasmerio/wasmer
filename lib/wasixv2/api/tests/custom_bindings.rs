//! Custom binding state belongs to each actual main/side instance, independent of its ABI.
#![cfg(all(feature = "sys", feature = "tokio", not(target_arch = "wasm32")))]

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use wasixv2::{
    ExecEnv, InstanceAttachment, InstantiationHook, Machine, MachineSyscalls, ProcessConfig, Stdio,
};
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, Global, Imports, Instance, Memory, Module, RuntimeError,
    StoreMut, Value,
};

type Events = Arc<Mutex<Vec<(usize, &'static str)>>>;
type ThreadEvents = Arc<Mutex<Vec<(&'static str, std::thread::ThreadId)>>>;
#[derive(Clone)]
struct ThreadProbe {
    events: ThreadEvents,
    barrier: Option<Arc<std::sync::Barrier>>,
}
impl ThreadProbe {
    fn record(&self, phase: &'static str) {
        self.events
            .lock()
            .unwrap()
            .push((phase, std::thread::current().id()));
    }
}
struct Hook {
    next: AtomicUsize,
    events: Events,
    omit: bool,
    thread: Option<ThreadProbe>,
}
struct State {
    id: usize,
    configured: bool,
    events: Events,
    thread: Option<ThreadProbe>,
    cleanup: Option<std::sync::Weak<wasixv2_host::LoaderCleanup>>,
}
struct Attachment {
    env: FunctionEnv<State>,
    exec: FunctionEnv<ExecEnv<MachineSyscalls>>,
}
impl InstantiationHook<MachineSyscalls> for Hook {
    fn requires_owning_thread(&self) -> bool {
        self.thread.is_some()
    }

    fn provides_import(&self, namespace: &str, name: &str) -> bool {
        namespace == "env" && matches!(name, "value" | "binding_bias")
    }

    fn prepare_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        exec: &FunctionEnv<ExecEnv<MachineSyscalls>>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
        if let Some(thread) = &self.thread {
            thread.record("inspect");
        }
        let cleanup = exec.as_ref(store).loader_cleanup().map(Arc::downgrade);
        let requested = module
            .imports()
            .filter(|import| {
                import.module() == "custom" || self.provides_import(import.module(), import.name())
            })
            .map(|import| {
                (
                    import.module().to_string(),
                    import.name().to_string(),
                    matches!(import.ty(), wasmer::ExternType::Global(_)),
                )
            })
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return Ok(None);
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push((id, "prepare"));
        if let Some(thread) = &self.thread {
            thread.record("prepare");
        }
        let env = FunctionEnv::new(
            store,
            State {
                id,
                configured: false,
                events: self.events.clone(),
                thread: self.thread.clone(),
                cleanup,
            },
        );
        for (namespace, name, global) in requested {
            if self.omit || imports.get_export(&namespace, &name).is_some() {
                continue;
            }
            if global {
                imports.define(&namespace, &name, Global::new(store, Value::I32(41)));
                continue;
            }
            imports.define(
                &namespace,
                &name,
                Function::new_typed_with_env(
                    store,
                    &env,
                    |env: FunctionEnvMut<State>| -> Result<i32, RuntimeError> {
                        if !env.data().configured {
                            return Err(RuntimeError::new("binding not configured"));
                        }
                        env.data()
                            .events
                            .lock()
                            .unwrap()
                            .push((env.data().id, "call"));
                        if let Some(thread) = &env.data().thread {
                            thread.record("call");
                            if let Some(barrier) = &thread.barrier {
                                barrier.wait();
                            }
                        }
                        Ok(env.data().id as i32)
                    },
                ),
            );
        }
        Ok(Some(Box::new(Attachment {
            env,
            exec: exec.clone(),
        })))
    }
}
impl InstanceAttachment for Attachment {
    fn configure(
        &mut self,
        _: &Module,
        store: &mut StoreMut<'_>,
        _: &Instance,
        _: &Memory,
    ) -> Result<(), RuntimeError> {
        let _ = self.exec.as_ref(&*store).task();
        let state = self.env.as_mut(store);
        state.configured = true;
        state.events.lock().unwrap().push((state.id, "configure"));
        if let Some(thread) = &state.thread {
            thread.record("configure");
        }
        Ok(())
    }
    fn deactivate(&mut self, store: &mut StoreMut<'_>) {
        let _ = self.exec.as_ref(&*store).task();
        let state = self.env.as_mut(store);
        state.configured = false;
        state.events.lock().unwrap().push((state.id, "deactivate"));
        if let Some(thread) = &state.thread {
            thread.record("deactivate");
            let thread = thread.clone();
            let scope = state.cleanup.as_ref().unwrap().upgrade().unwrap();
            assert!(
                scope
                    .submit(Box::new(move || Box::pin(async move {
                        thread.record("cleanup");
                    })))
                    .is_ok()
            );
        }
    }
}

fn leb(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            break;
        }
    }
}
// Match api::retirement and wasix1::dynamic's source-built dylink.0 fixtures.
fn pic(wat: &str, needed: &[&str]) -> Vec<u8> {
    let mut wasm = wat::parse_str(wat).unwrap();
    let mut data = vec![8];
    data.extend_from_slice(b"dylink.0");
    let mut info = Vec::new();
    for value in [512, 0, 0, 0] {
        leb(value, &mut info);
    }
    data.push(1);
    leb(info.len() as u32, &mut data);
    data.extend(info);
    if !needed.is_empty() {
        let mut names = Vec::new();
        leb(needed.len() as u32, &mut names);
        for name in needed {
            leb(name.len() as u32, &mut names);
            names.extend_from_slice(name.as_bytes());
        }
        data.push(2);
        leb(names.len() as u32, &mut data);
        data.extend(names);
    }
    wasm.push(0);
    leb(data.len() as u32, &mut wasm);
    wasm.extend(data);
    wasm
}

struct Programs(PathBuf);
impl Drop for Programs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Custom,
    Env,
    Global,
    NamespacedStack,
    Guest,
    WrongGuest,
    Missing,
}

async fn run(name: &str, main_binding: bool) -> Events {
    let (result, events) = run_case(name, main_binding, Variant::Custom).await;
    assert_eq!(result.unwrap(), 0);
    events
}

async fn run_case(
    name: &str,
    main_binding: bool,
    variant: Variant,
) -> (Result<i32, wasixv2::RunError>, Events) {
    run_case_with_thread(name, main_binding, variant, None, false).await
}

async fn run_case_with_thread(
    name: &str,
    main_binding: bool,
    variant: Variant,
    thread: Option<ThreadProbe>,
    file: bool,
) -> (Result<i32, wasixv2::RunError>, Events) {
    let programs = Programs(std::env::temp_dir().join(format!(
        "wasixv2-custom-binding-{name}-{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&programs.0).unwrap();
    let namespace = if variant == Variant::Custom {
        "custom"
    } else {
        "env"
    };
    let side_import = if variant == Variant::NamespacedStack {
        "(import \"custom\" \"__stack_pointer\" (global $bias i32))".to_string()
    } else if variant == Variant::Global {
        "(import \"env\" \"binding_bias\" (global $bias i32))".to_string()
    } else {
        format!("(import \"{namespace}\" \"value\" (func $value (result i32)))")
    };
    let side_body = if matches!(variant, Variant::Global | Variant::NamespacedStack) {
        "global.get $bias"
    } else {
        "call $value"
    };
    let side = pic(
        &format!(
            r#"(module
        (import "env" "memory" (memory 1 65536 shared))
        (import "env" "__indirect_function_table" (table 0 funcref))
        (import "env" "__memory_base" (global i32))
        (import "env" "__table_base" (global i32))
        {side_import}
        (func (export "side_value") (result i32) {side_body}))"#
        ),
        &[],
    );
    std::fs::write(programs.0.join("side.so"), side).unwrap();
    let custom = if main_binding {
        "(import \"custom\" \"value\" (func $value (result i32)))"
    } else {
        ""
    };
    let expected = match variant {
        Variant::Global | Variant::NamespacedStack => 41,
        Variant::Guest => 79,
        _ => 1,
    };
    let body = if main_binding {
        "call $value call $side i32.eq".to_string()
    } else {
        format!("call $side i32.const {expected} i32.ne")
    };
    let guest = match variant {
        Variant::Guest => "(func (export \"value\") (result i32) i32.const 79)",
        Variant::WrongGuest => "(func (export \"value\") (result i64) i64.const 79)",
        _ => "",
    };
    let main = pic(
        &format!(
            r#"(module
        (import "env" "memory" (memory 1 65536 shared))
        (import "env" "__indirect_function_table" (table 3 funcref))
        (import "env" "__stack_pointer" (global (mut i32)))
        (import "env" "__memory_base" (global i32))
        (import "env" "__table_base" (global i32))
        (import "env" "side_value" (func $side (result i32)))
        (import "wasixv2" "exit_group" (func $exit (param i32)))
        {custom}
        {guest}
        (export "__indirect_function_table" (table 0))
        (func (export "_start") {body} call $exit))"#
        ),
        &["side.so"],
    );
    if file {
        let path = programs.0.join("main.wasm");
        std::fs::write(&path, &main).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let machine = Machine::builder()
        .instantiation_hook(Arc::new(Hook {
            next: AtomicUsize::new(1),
            events: events.clone(),
            omit: variant == Variant::Missing,
            thread,
        }))
        .build();
    machine
        .mount_host_dir("/prog", &programs.0, false)
        .await
        .unwrap();
    let config = ProcessConfig {
        args: vec!["custom-binding".into()],
        env: vec!["LD_LIBRARY_PATH=/prog".into()],
        stdio: Stdio::from_fns(|_| {}, |_| {}),
    };
    let status = if file {
        machine
            .run_file(&wasmer::Engine::default(), "/prog/main.wasm", config)
            .await
    } else {
        machine.run_process(&main, config).await
    };
    (status, events)
}

#[tokio::test]
async fn main_and_side_module_use_distinct_bindings_in_the_same_store() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let events = run("paired", true).await;
            let events = events.lock().unwrap();
            let calls: Vec<_> = events
                .iter()
                .filter(|(_, event)| *event == "call")
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(calls.len(), 2);
            assert_ne!(calls[0], calls[1]);
            let retired: Vec<_> = events
                .iter()
                .filter(|(_, event)| *event == "deactivate")
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(retired, [2, 1]);
        })
        .await;
}

#[tokio::test]
async fn side_module_only_imports_get_their_own_custom_binding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let events = run("side-only", false).await;
            assert_eq!(
                *events.lock().unwrap(),
                [
                    (1, "prepare"),
                    (1, "configure"),
                    (1, "call"),
                    (1, "deactivate")
                ]
            );
        })
        .await;
}

#[tokio::test]
async fn declared_env_helpers_and_globals_reach_real_side_module_preparation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (name, variant) in [
                ("declared-env", Variant::Env),
                ("declared-global", Variant::Global),
                ("namespaced-stack", Variant::NamespacedStack),
            ] {
                let (result, events) = run_case(name, false, variant).await;
                assert_eq!(result.unwrap(), 0);
                let events = events.lock().unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|(_, event)| *event == "configure")
                        .count(),
                    1
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|(_, event)| *event == "deactivate")
                        .count(),
                    1
                );
            }
        })
        .await;
}

#[tokio::test]
async fn guest_definition_precedes_declared_host_fallback_and_type_errors_stay_errors() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (result, events) = run_case("guest-precedence", false, Variant::Guest).await;
            assert_eq!(result.unwrap(), 0);
            assert!(
                !events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, event)| *event == "call")
            );
            let (result, events) = run_case("guest-type-error", false, Variant::WrongGuest).await;
            assert!(
                !matches!(result, Ok(0)),
                "a matching guest definition's type mismatch must not fall back to the host"
            );
            assert!(events.lock().unwrap().is_empty());
        })
        .await;
}

#[tokio::test]
async fn declaration_without_a_prepared_value_fails_admission_and_deactivates() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (result, events) = run_case("missing-declared", false, Variant::Missing).await;
            assert!(
                !matches!(result, Ok(0)),
                "a declaration must never synthesize an import value"
            );
            assert_eq!(*events.lock().unwrap(), [(1, "prepare"), (1, "deactivate")]);
        })
        .await;
}

// Match the API's async_sync_borrow owned subprocess boundary: a broken
// factory could block the caller on the two-root meeting barrier.
fn owning_thread_subprocess(name: &str) -> bool {
    const CHILD: &str = "WASIXV2_OWNING_THREAD_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnedChild(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, "1")
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match child.0.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "owning-thread proof failed: {status}");
                return false;
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            result => panic!("owning-thread proof did not finish: {result:?}"),
        }
    }
}

#[tokio::test]
async fn side_only_bindings_construct_and_retire_on_their_owning_thread() {
    if !owning_thread_subprocess("side_only_bindings_construct_and_retire_on_their_owning_thread") {
        return;
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for (name, file) in [("owner-side-bytes", false), ("owner-side-file", true)] {
                let caller = std::thread::current().id();
                let events: ThreadEvents = Arc::default();
                let (result, _) = run_case_with_thread(
                    name,
                    false,
                    Variant::Env,
                    Some(ThreadProbe {
                        events: events.clone(),
                        barrier: None,
                    }),
                    file,
                )
                .await;
                assert_eq!(result.unwrap(), 0);
                let events = events.lock().unwrap();
                let owner = events
                    .iter()
                    .find(|(phase, _)| *phase == "prepare")
                    .unwrap()
                    .1;
                assert_ne!(owner, caller);
                assert!(events.iter().all(|(_, thread)| *thread == owner));
                assert!(
                    events
                        .iter()
                        .filter(|(phase, _)| *phase == "inspect")
                        .count()
                        >= 2
                );
                assert_eq!(events.last().unwrap().0, "cleanup");
                for phase in ["prepare", "configure", "call", "deactivate", "cleanup"] {
                    assert_eq!(events.iter().filter(|(seen, _)| *seen == phase).count(), 1);
                }
            }
        })
        .await;
}

#[tokio::test]
async fn independent_binding_roots_use_distinct_owning_threads() {
    if !owning_thread_subprocess("independent_binding_roots_use_distinct_owning_threads") {
        return;
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let first: ThreadEvents = Arc::default();
            let second: ThreadEvents = Arc::default();
            let (left, right) = tokio::join!(
                run_case_with_thread(
                    "owner-first",
                    false,
                    Variant::Env,
                    Some(ThreadProbe {
                        events: first.clone(),
                        barrier: Some(barrier.clone()),
                    }),
                    false
                ),
                run_case_with_thread(
                    "owner-second",
                    false,
                    Variant::Env,
                    Some(ThreadProbe {
                        events: second.clone(),
                        barrier: Some(barrier),
                    }),
                    false
                ),
            );
            assert_eq!(left.0.unwrap(), 0);
            assert_eq!(right.0.unwrap(), 0);
            let first = first.lock().unwrap();
            let second = second.lock().unwrap();
            let left = first.iter().find(|(phase, _)| *phase == "call").unwrap().1;
            let right = second.iter().find(|(phase, _)| *phase == "call").unwrap().1;
            assert_ne!(left, right);
            assert_eq!(first.last().unwrap().0, "cleanup");
            assert_eq!(second.last().unwrap().0, "cleanup");
        })
        .await;
}
