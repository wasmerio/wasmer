//! Shared-runtime browser assembly. JS bootstrap must finish before entering this API;
//! all guest calls run synchronously on coordinator-created task workers.
pub mod relay;

use crate::{Host, MachineBuilder, Net};
use std::{io, sync::Arc};
use wasixv2_host::browser::{BrowserExecutor, ObjectHooks, set_object_hooks};
use wasm_bindgen::prelude::*;

pub use wasixv2_stdio_web::{Session, Streams};
pub use wasixv2_vfs_web::{WebFs, memory as directory};
pub use wasixv2_vnet_web::{Capabilities, Config as NetworkLimits, Driver, WebBackend};

/// Install on **every** worker via bootstrap's `configure` option, before importing
/// the admission envelope. Handles refer to objects in this one shared runtime only.
#[wasm_bindgen]
pub fn configure_browser() {
    set_object_hooks(ObjectHooks {
        prepare: || {
            Ok(wasmer::js::prepare_shared_object_message(
                JsValue::UNDEFINED,
            ))
        },
        receive: |message| {
            // SAFETY: only B2's trusted coordinator connects workers sharing this heap.
            unsafe {
                wasmer::js::receive_shared_object_message(message)?;
            }
            Ok(())
        },
        collect: wasmer::js::collect_shared_objects,
        teardown: wasmer::js::collect_shared_objects,
    });
}

/// Kernel drivers use `host` (configured with `executor.services()`), and guest
/// threads use the task executor. The host must use off-worker clocks/timers.
/// Each call boots a distinct kernel; share directories explicitly through mounts.
pub fn builder(
    host: Arc<dyn Host>,
    executor: BrowserExecutor,
    network: Arc<Net>,
) -> MachineBuilder {
    MachineBuilder::new(host)
        .task_executor(Arc::new(executor))
        .network(network)
}

/// A backend/driver pair for an embedder's independent event-loop transport. No
/// WebSocket, fetch or provider Promise is polled on a blocking guest worker.
pub fn network(
    config: crate::NetConfig,
    limits: NetworkLimits,
    capabilities: Capabilities,
) -> io::Result<(Arc<Net>, Driver)> {
    let (backend, driver) = WebBackend::new(limits, capabilities)
        .map_err(|errno| io::Error::from_raw_os_error(errno.0))?;
    Ok((Net::new(config, Some(backend)), driver))
}

use crate::{Engine, Machine, ProcessConfig};
use std::sync::Mutex;
use wasixv2_host::Executor;
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};
use wasmer::Module;

#[derive(Default)]
struct Completion {
    result: Mutex<Option<Result<i32, String>>>,
    changed: WaitQueue,
}

/// One sandbox run. Keep the Machine private to this run when using `kill`:
/// it terminates the Machine's complete process tree, including fork descendants.
/// The SDK owns JS Promise cancellation; coordinator infrastructure failure must
/// reject every JS waiter rather than reuse the failed shared heap.
pub struct Run {
    machine: Arc<Machine>,
    completion: Arc<Completion>,
}

impl Run {
    /// Wait repeatedly/concurrently for the final kernel exit status.
    pub async fn wait(&self) -> Result<i32, String> {
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.completion.changed, PollMask::empty());
        loop {
            if let Some(result) = self.completion.result.lock().unwrap().clone() {
                return result;
            }
            waiter.sleep(&NoInterrupt, None).await;
        }
    }
    /// Kill and reap the sandbox's running process tree before resolving.
    /// Linux kernel/exit.c:do_exit and exit_mm remain the only guest teardown path.
    pub async fn kill(&self) -> Result<i32, String> {
        self.machine.shutdown().await;
        self.wait().await
    }
    pub fn signaller(&self) -> crate::Signaller {
        self.machine.signaller()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let running = self.completion.result.lock().unwrap().is_none();
        if running {
            let _ = self.machine.signal(9);
        }
    }
}

/// Start on a dedicated guest worker, delivering Module/Memory handles through
/// the configured B2 envelope hooks. No Store or instantiated guest crosses workers.
/// Output must be consumed concurrently; after wait, shut down descendants before
/// sealing Session outputs, and continue draining until EOF.
pub async fn spawn(
    machine: Arc<Machine>,
    executor: &BrowserExecutor,
    engine: Engine,
    module: Module,
    config: ProcessConfig,
) -> Result<Run, crate::RunError> {
    // Publish the kernel task before returning the handle, so an immediate kill
    // cannot race an as-yet-uncreated process (kernel/fork.c:kernel_clone and kernel/sched/core.c:wake_up_new_task).
    crate::preflight(&module)?;
    let (task, admission) = machine.start(config).await?;
    let completion = Arc::new(Completion::default());
    let output = completion.clone();
    let running = machine.clone();
    executor
        .spawn_thread(Box::new(move || {
            Box::pin(async move {
                let guard = crate::Running {
                    machine: &running,
                    task,
                    released: false,
                };
                let result = running
                    .run_image(wasmer::Store::new(engine), &module, task)
                    .await
                    .map_err(|e| e.to_string());
                guard.released();
                running.console_sent().await;
                *output.result.lock().unwrap() = Some(result);
                output.changed.wake(PollMask::empty());
            })
        }))
        .map_err(crate::RunError::Spawn)?;
    // Transfer the existing running-list entry to the worker guard. Calling
    // released here would remove it before Signaller can target the guest.
    std::mem::forget(admission);
    Ok(Run {
        machine,
        completion,
    })
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = warn)]
    pub(super) fn report(message: &str);
}
