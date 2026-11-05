//! Optional native N-API bindings paired with each actual instance.

use std::sync::{Arc, Mutex, Weak};
use wasixv2_exec::{ExecEnv, GuestMemory, InstanceAttachment, InstantiationHook};
use wasixv2_host::LoaderCleanup;
use wasixv2_syscalls::{Caller, HostMemoryLease, LoaderControl};
use wasmer::{FunctionEnv, Imports, Instance, Memory, Module, RuntimeError, StoreMut};
use wasmer_napi::{NapiRuntimeHooks, NapiSession, NativeGuestChunk, NativeGuestRefill};

use crate::MachineSyscalls;
pub use wasmer_napi::{NapiCtx, NapiCtxBuilder, NapiLimits, NapiRuntimeControl};

/// Registers the native binding independently of the process ABI.
pub struct NapiHook {
    context: NapiCtx,
    hooks: NapiRuntimeHooks,
}
impl NapiHook {
    pub fn new(context: NapiCtx) -> Self {
        Self {
            hooks: context.runtime_hooks(),
            context,
        }
    }
}

struct Attachment {
    context: NapiCtx,
    session: NapiSession,
    exec: FunctionEnv<ExecEnv<MachineSyscalls>>,
}

impl InstantiationHook<MachineSyscalls> for NapiHook {
    fn requires_owning_thread(&self) -> bool {
        true
    }

    fn provides_import(&self, namespace: &str, name: &str) -> bool {
        NapiCtx::provides_import(namespace, name)
    }

    fn prepare_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        exec: &FunctionEnv<ExecEnv<MachineSyscalls>>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
        let state = self
            .hooks
            .add_imports(module, store, imports)
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        let Some(session) = state.session().cloned() else {
            return Ok(None);
        };
        Ok(Some(Box::new(Attachment {
            context: self.context.clone(),
            session,
            exec: exec.clone(),
        })))
    }
}

impl InstanceAttachment for Attachment {
    fn configure(
        &mut self,
        _: &Module,
        store: &mut StoreMut<'_>,
        instance: &Instance,
        memory: &Memory,
    ) -> Result<(), RuntimeError> {
        let arena = self
            .context
            .native_guest_arena(&*store, memory)
            .ok_or_else(|| RuntimeError::new("native N-API requires stable shared guest memory"))?;
        self.session
            .configure_leased_instance(
                store,
                instance,
                memory,
                arena,
                Arc::new(Refill {
                    exec: Mutex::new(self.exec.clone()),
                }),
            )
            .map_err(|error| RuntimeError::new(error.to_string()))
    }

    fn deactivate(&mut self, store: &mut StoreMut<'_>) {
        // The existing attachment guard settles native generations and takes
        // the original Store's write custody before calling this on its owner.
        unsafe { self.session.deactivate(store) };
    }
}

struct Refill {
    exec: Mutex<FunctionEnv<ExecEnv<MachineSyscalls>>>,
}
// SAFETY: FunctionEnv is an exact Store-local handle, not an owning Store.
// Only the current operation captures an async loan; it is never retained here.
unsafe impl NativeGuestRefill for Refill {
    fn reserve(
        &self,
        store: &mut StoreMut<'_>,
        bytes: u32,
    ) -> Result<Option<NativeGuestChunk>, RuntimeError> {
        // Match InstanceHooks' store-local handle custody. Only clone under
        // this lock; no Store access, callout or suspension holds its guard.
        let exec = self
            .exec
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if !wasmer::sys::native_host_work_can_suspend() {
            let cleanup = exec.as_ref(&*store).loader_cleanup().cloned();
            let Some(cleanup) = cleanup else {
                return Ok(None);
            };
            return match wasixv2_exec::poll_native_host_reservation(&exec, store, bytes) {
                std::task::Poll::Ready(Ok(lease)) => Ok(Some(chunk(lease, cleanup))),
                std::task::Poll::Ready(Err(_)) | std::task::Poll::Pending => Ok(None),
            };
        }
        let (syscalls, task, cleanup) = {
            let data = exec.as_ref(&*store);
            let Some(cleanup) = data.loader_cleanup().cloned() else {
                return Ok(None);
            };
            (data.syscalls().clone(), data.task(), cleanup)
        };
        // Install the exact borrowed suffix first. Capture its checked async
        // loan only through a counted temporary; release all T/frame/Store
        // wrappers before the driver can yield this suffix.
        store.parked(|| {
            let env = wasmer::Store::with_current(|current| {
                exec.clone().into_mut(current).as_async_mut()
            })
            .flatten();
            let Some(env) = env else {
                return Ok(None);
            };
            wasmer::sys::run_native_host_future(async move {
                let memory = GuestMemory::new(&env);
                let caller = Caller::new(task, &memory);
                let Ok(lease) = syscalls.loader_reserve_host(&caller, bytes, 65536).await else {
                    return Ok(None);
                };
                Ok(Some(chunk(lease, cleanup)))
            })
        })
    }
}

fn chunk(lease: Box<dyn HostMemoryLease>, cleanup: Arc<LoaderCleanup>) -> NativeGuestChunk {
    let address = lease.address();
    let length = lease.length();
    let pointer = lease.as_ptr();
    let owner = Box::new(LeaseOwner {
        lease: Some(lease),
        cleanup: Arc::downgrade(&cleanup),
    });
    // SAFETY: the producer pairs exact private logical-range and native pin
    // custody. LeaseOwner retains both through the final backing-store Drop.
    unsafe { NativeGuestChunk::new(address, length, pointer, owner) }
}

struct LeaseOwner {
    lease: Option<Box<dyn HostMemoryLease>>,
    cleanup: Weak<LoaderCleanup>,
}
impl Drop for LeaseOwner {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        // The closure crosses threads; its local future is constructed by the
        // original runner. Closed-scope rejection drops the exact lease, whose
        // contract requires safe host-only release without an unpolled job.
        let job = Box::new(move || lease.release());
        if let Some(cleanup) = self.cleanup.upgrade() {
            if let Err(job) = cleanup.submit(job) {
                drop(job);
            }
        } else {
            drop(job);
        }
    }
}
