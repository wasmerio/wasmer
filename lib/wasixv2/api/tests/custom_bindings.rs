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
    Function, FunctionEnv, FunctionEnvMut, Imports, Instance, Memory, Module, RuntimeError,
    StoreMut,
};

type Events = Arc<Mutex<Vec<(usize, &'static str)>>>;
struct Hook {
    next: AtomicUsize,
    events: Events,
}
struct State {
    id: usize,
    configured: bool,
    events: Events,
}
struct Attachment {
    env: FunctionEnv<State>,
}
impl InstantiationHook<MachineSyscalls> for Hook {
    fn prepare_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        _: &FunctionEnv<ExecEnv<MachineSyscalls>>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
        if !module.imports().any(|import| import.module() == "custom") {
            return Ok(None);
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push((id, "prepare"));
        let env = FunctionEnv::new(
            store,
            State {
                id,
                configured: false,
                events: self.events.clone(),
            },
        );
        imports.define(
            "custom",
            "value",
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
                    Ok(env.data().id as i32)
                },
            ),
        );
        Ok(Some(Box::new(Attachment { env })))
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
        let state = self.env.as_mut(store);
        state.configured = true;
        state.events.lock().unwrap().push((state.id, "configure"));
        Ok(())
    }
    fn deactivate(&mut self, store: &mut StoreMut<'_>) {
        let state = self.env.as_mut(store);
        state.configured = false;
        state.events.lock().unwrap().push((state.id, "deactivate"));
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

async fn run(name: &str, main_binding: bool) -> Events {
    let programs = Programs(std::env::temp_dir().join(format!(
        "wasixv2-custom-binding-{name}-{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&programs.0).unwrap();
    let side = pic(
        r#"(module
        (import "env" "memory" (memory 1 65536 shared))
        (import "env" "__indirect_function_table" (table 0 funcref))
        (import "env" "__memory_base" (global i32))
        (import "env" "__table_base" (global i32))
        (import "custom" "value" (func $value (result i32)))
        (func (export "side_value") (result i32) call $value))"#,
        &[],
    );
    std::fs::write(programs.0.join("side.so"), side).unwrap();
    let custom = if main_binding {
        "(import \"custom\" \"value\" (func $value (result i32)))"
    } else {
        ""
    };
    let body = if main_binding {
        "call $value call $side i32.eq"
    } else {
        "call $side i32.const 1 i32.ne"
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
        (export "__indirect_function_table" (table 0))
        (func (export "_start") {body} call $exit))"#
        ),
        &["side.so"],
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let machine = Machine::builder()
        .instantiation_hook(Arc::new(Hook {
            next: AtomicUsize::new(1),
            events: events.clone(),
        }))
        .build();
    machine
        .mount_host_dir("/prog", &programs.0, false)
        .await
        .unwrap();
    let status = machine
        .run_process(
            &main,
            ProcessConfig {
                args: vec!["custom-binding".into()],
                env: vec!["LD_LIBRARY_PATH=/prog".into()],
                stdio: Stdio::from_fns(|_| {}, |_| {}),
            },
        )
        .await
        .unwrap();
    assert_eq!(status, 0);
    events
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
