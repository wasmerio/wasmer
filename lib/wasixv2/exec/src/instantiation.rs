//! Ordered, store-local custom bindings, independent of the selected process ABI.

use std::sync::{Arc, Mutex};
use wasmer::{
    AsStoreAsync, AsStoreMut, FunctionEnv, Imports, Instance, Memory, Module, RuntimeError, Store,
    StoreAsync, StoreMut,
};

use crate::ExecEnv;

/// Adds imports after ABI preparation. Existing imports must remain unchanged.
/// Each successful preparation returns state belonging to this exact instance and Store.
/// An engine-level Wasm `start` can call imports before configuration: imports that
/// need the bound instance must reject calls until their attachment is configured.
/// A hook must not retain an owning Store/StoreAsync, including indirectly: images
/// retain registered hooks for declarations and their Stores retain bound hook state.
pub trait InstantiationHook<S>: Send + Sync + 'static {
    /// Requires each initial/native task Store to be constructed inside the
    /// executor's owning-thread factory. This fixed registry requirement also
    /// applies when only a later side module imports the binding.
    fn requires_owning_thread(&self) -> bool {
        false
    }

    /// Declares host imports that a dynamic linker may otherwise treat as missing
    /// guest symbols. This pure declaration must stay fixed for the image lifetime;
    /// it neither constructs a value nor overrides infrastructure or a guest definition.
    fn provides_import(&self, _namespace: &str, _name: &str) -> bool {
        false
    }

    /// Host-only preparation: do not call guest functions or retain borrowed Store access.
    /// On failure, the hook must clean up its own partial preparation.
    fn prepare_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        env: &FunctionEnv<ExecEnv<S>>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError>;
}

/// Owned state paired with one hook's preparation and one actual instance.
pub trait InstanceAttachment: Send + 'static {
    /// Host-only setup before ABI entry and its explicit initializers.
    /// Attachments must not retain an owning Store or StoreAsync, directly or through
    /// another resource: their Store already retains the prepared state.
    fn configure(
        &mut self,
        module: &Module,
        store: &mut StoreMut<'_>,
        instance: &Instance,
        memory: &Memory,
    ) -> Result<(), RuntimeError>;

    /// Release Store-owned roots without guest callbacks. Called on the owning runner,
    /// including after prepare/configure failure; must tolerate partial configuration.
    fn deactivate(&mut self, store: &mut StoreMut<'_>);
}

/// Owns only native generation metadata, outside the Store's ExecEnv.
/// The existing attachment guard transfers it with cleanup on cancellation.
#[derive(Default)]
pub(crate) struct NativeInvocations {
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    inner: Option<wasmer::sys::NativeInvocationScope>,
}

impl NativeInvocations {
    pub(crate) fn bind(store: &mut Store, required: bool) -> Result<Self, crate::ExecError> {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        let native = store.engine().is_sys();
        #[cfg(not(all(feature = "sys", not(target_arch = "wasm32"))))]
        let native = false;
        if !required && !native {
            return Ok(Self::default());
        }
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        {
            wasmer::sys::NativeInvocationScope::bind(store)
                .map(|inner| Self { inner: Some(inner) })
                .map_err(crate::ExecError::StoreMode)
        }
        #[cfg(not(all(feature = "sys", not(target_arch = "wasm32"))))]
        {
            let _ = store;
            Err(crate::ExecError::OwningThreadUnsupported)
        }
    }

    pub(crate) fn is_some(&self) -> bool {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        {
            self.inner.is_some()
        }
        #[cfg(not(all(feature = "sys", not(target_arch = "wasm32"))))]
        {
            false
        }
    }

    pub(crate) fn close(&self) {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        if let Some(inner) = &self.inner {
            inner.close();
        }
    }

    async fn settle(&mut self) {
        #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
        if let Some(inner) = &mut self.inner {
            // This observer belongs to the runner's external cleanup custody.
            // Its poll may be nested in a foreign executor Waker callback.
            inner.close_and_wait_for_cleanup().await;
        }
    }
}

/// Store-independent declarations retained by an image's symbol planner.
#[derive(Clone)]
pub struct ImportDeclarations {
    hooks: Arc<Vec<Arc<dyn Declaration>>>,
    identities: Arc<Vec<usize>>,
}
trait Declaration: Send + Sync {
    fn provides_import(&self, namespace: &str, name: &str) -> bool;
}
struct Declared<S>(Arc<dyn InstantiationHook<S>>);
impl<S: 'static> Declaration for Declared<S> {
    fn provides_import(&self, namespace: &str, name: &str) -> bool {
        self.0.provides_import(namespace, name)
    }
}
impl ImportDeclarations {
    fn new<S: 'static>(hooks: &[Arc<dyn InstantiationHook<S>>]) -> Self {
        Self {
            identities: Arc::new(
                hooks
                    .iter()
                    .map(|hook| Arc::as_ptr(hook) as *const () as usize)
                    .collect(),
            ),
            hooks: Arc::new(
                hooks
                    .iter()
                    .map(|hook| Arc::new(Declared(hook.clone())) as Arc<dyn Declaration>)
                    .collect(),
            ),
        }
    }
    pub fn provides_import(&self, namespace: &str, name: &str) -> bool {
        self.hooks
            .iter()
            .any(|hook| hook.provides_import(namespace, name))
    }
    /// Whether peer Stores use the exact same ordered registration objects.
    pub fn same_hooks(&self, other: &Self) -> bool {
        self.identities == other.identities
    }
}

type Cell = Arc<Mutex<Option<Box<dyn InstanceAttachment>>>>;

/// The custom bindings of one actual instance. Clones share idempotent deactivation.
#[derive(Clone, Default)]
pub struct InstanceAttachments {
    cells: Vec<Cell>,
}

impl InstanceAttachments {
    pub fn configure(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        instance: &Instance,
        memory: &Memory,
    ) -> Result<(), RuntimeError> {
        for cell in &self.cells {
            if let Some(attachment) = cell.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                attachment.configure(module, store, instance, memory)?;
            }
        }
        Ok(())
    }

    pub fn deactivate(&self, store: &mut StoreMut<'_>) {
        let mut panics = Vec::new();
        for cell in self.cells.iter().rev() {
            let attachment = cell.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(mut attachment) = attachment {
                if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    attachment.deactivate(store)
                })) {
                    panics.push(panic);
                }
                if let Err(panic) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(attachment)))
                {
                    panics.push(panic);
                }
            }
        }
        if !panics.is_empty() {
            let first = panics.remove(0);
            // All attachments have settled. Recursive panics from adversarial payload
            // destructors follow ordinary Rust panic behavior.
            for panic in panics {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(panic)));
            }
            std::panic::resume_unwind(first);
        }
    }
}

/// A store's hook registry. It retains successful state through unpublished staging,
/// and contains only store-local handles, never an owning Store reference.
#[derive(Clone)]
pub struct InstanceHooks {
    declarations: ImportDeclarations,
    hooks: Vec<Arc<dyn BoundHook>>,
    attached: Arc<Mutex<InstanceAttachments>>,
}

trait BoundHook: Send + Sync {
    fn prepare(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError>;
}

struct Hook<S> {
    hook: Arc<dyn InstantiationHook<S>>,
    env: Mutex<FunctionEnv<ExecEnv<S>>>,
}

impl<S: Send + Sync + 'static> BoundHook for Hook<S> {
    fn prepare(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        imports: &mut Imports,
    ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
        let env = self.env.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.hook.prepare_imports(module, store, &env, imports)
    }
}

impl InstanceHooks {
    pub(crate) fn new<S: Send + Sync + 'static>(
        hooks: Vec<Arc<dyn InstantiationHook<S>>>,
        env: FunctionEnv<ExecEnv<S>>,
    ) -> Self {
        Self {
            declarations: ImportDeclarations::new(&hooks),
            hooks: hooks
                .into_iter()
                .map(|hook| {
                    Arc::new(Hook {
                        hook,
                        env: Mutex::new(env.clone()),
                    }) as Arc<dyn BoundHook>
                })
                .collect(),
            attached: Arc::default(),
        }
    }

    /// Declarations contain only original registered hooks, never this Store's env.
    pub fn declarations(&self) -> ImportDeclarations {
        self.declarations.clone()
    }

    pub fn prepare(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        imports: &mut Imports,
    ) -> Result<InstanceAttachments, RuntimeError> {
        self.attached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cells
            .retain(|cell| cell.lock().unwrap_or_else(|e| e.into_inner()).is_some());
        let mut prepared = InstanceAttachments::default();
        for hook in &self.hooks {
            let prior = imports.clone();
            let result = hook.prepare(module, store, imports);
            match result {
                Ok(Some(attachment)) => {
                    let cell = Arc::new(Mutex::new(Some(attachment)));
                    self.attached
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .cells
                        .push(cell.clone());
                    prepared.cells.push(cell);
                }
                Ok(None) => (),
                Err(error) => {
                    prepared.deactivate(store);
                    return Err(error);
                }
            }
            if prior.into_iter().any(|((module, name), value)| {
                imports.get_export(&module, &name).as_ref() != Some(&value)
            }) {
                prepared.deactivate(store);
                return Err(RuntimeError::new(
                    "an instantiation hook replaced an earlier import",
                ));
            }
        }
        Ok(prepared)
    }

    pub(crate) fn deactivate(&self, store: &mut StoreMut<'_>) {
        let attached =
            std::mem::take(&mut *self.attached.lock().unwrap_or_else(|e| e.into_inner()));
        attached.deactivate(store);
    }

    #[cfg(test)]
    pub(crate) fn guard(
        &self,
        store: &StoreAsync,
        scope: Arc<wasixv2_host::LoaderCleanup>,
    ) -> AttachmentGuard {
        AttachmentGuard::new(
            Some(self.clone()),
            store,
            scope,
            NativeInvocations::default(),
        )
    }
}

/// Cancellation transfers the actual state and Store to the runner's cleanup queue.
pub(crate) struct AttachmentGuard {
    hooks: Option<InstanceHooks>,
    store: StoreAsync,
    scope: Arc<wasixv2_host::LoaderCleanup>,
    invocations: NativeInvocations,
    settled: bool,
    contexts: Option<Arc<crate::contexts::Shared>>,
    outcome: Option<Result<(), RuntimeError>>,
}

impl AttachmentGuard {
    pub(crate) fn for_store(
        hooks: Option<InstanceHooks>,
        store: &StoreAsync,
        scope: Arc<wasixv2_host::LoaderCleanup>,
        invocations: NativeInvocations,
    ) -> Option<Self> {
        if hooks.is_none() && !invocations.is_some() {
            return None;
        }
        Some(Self::new(hooks, store, scope, invocations))
    }

    pub(crate) fn new(
        hooks: Option<InstanceHooks>,
        store: &StoreAsync,
        scope: Arc<wasixv2_host::LoaderCleanup>,
        invocations: NativeInvocations,
    ) -> Self {
        let settled = !invocations.is_some();
        Self {
            hooks,
            store: store.store(),
            scope,
            invocations,
            settled,
            contexts: None,
            outcome: None,
        }
    }

    pub(crate) fn retain_contexts(&mut self, contexts: Arc<crate::contexts::Shared>) {
        assert!(self.contexts.replace(contexts).is_none());
    }

    pub(crate) async fn finish(
        &mut self,
        result: Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        // Own the cooperative driver's outcome before any pending fence. Drop
        // transfers it with this same guard if the runner is canceled here.
        self.outcome = Some(result);
        self.settle().await;
        let result = self.outcome.take().expect("runner outcome retained");
        match self.contexts.take() {
            Some(contexts) => contexts.resolve(result),
            None => result,
        }
    }

    pub(crate) async fn settle(&mut self) {
        #[cfg(feature = "js")]
        if let Some(contexts) = &self.contexts {
            crate::contexts::jspi::settle(contexts).await;
        }
        if self.settled {
            return;
        }
        self.invocations.settle().await;
        // Physical settlement notification precedes the final original unlock.
        // Actual write custody fences every attachment and memory retirement.
        drop(self.store.write_lock().await);
        self.settled = true;
    }

    pub(crate) async fn deactivate(&mut self) {
        self.settle().await;
        let mut store = self.store.write_lock().await;
        if let Some(hooks) = self.hooks.take() {
            hooks.deactivate(&mut store.as_store_mut());
        }
        self.invocations = NativeInvocations::default();
        drop(store);
        drop(self.contexts.take());
        drop(self.outcome.take());
    }

    pub(crate) fn disarm(&mut self) -> NativeInvocations {
        self.hooks = None;
        std::mem::take(&mut self.invocations)
    }
}

impl Drop for AttachmentGuard {
    fn drop(&mut self) {
        let hooks = self.hooks.take();
        let invocations = std::mem::take(&mut self.invocations);
        let contexts = self.contexts.take();
        let outcome = self.outcome.take();
        let attached = hooks.as_ref().is_some_and(|hooks| {
            hooks
                .attached
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cells
                .iter()
                .any(|cell| cell.lock().unwrap_or_else(|e| e.into_inner()).is_some())
        });
        if !attached && !invocations.is_some() && contexts.is_none() && outcome.is_none() {
            return;
        }
        let scoped = invocations.is_some() || contexts.is_some() || outcome.is_some();
        let unwinding = std::thread::panicking();
        let closing = if scoped {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invocations.close()))
        } else {
            Ok(())
        };
        if !scoped {
            let store = self.store.store();
            self.scope.spawn(Box::pin(async move {
                let mut store = store.write_lock().await;
                if let Some(hooks) = hooks {
                    hooks.deactivate(&mut store.as_store_mut());
                }
            }));
            return;
        }
        // Keep cancellation custody inside the queued continuation too. A valid
        // waiter callback can panic while polling it: dropping that future then
        // transfers this same guard before its old job completion is released.
        let mut pending = Self {
            hooks,
            store: self.store.store(),
            scope: self.scope.clone(),
            invocations,
            settled: self.settled,
            contexts,
            outcome,
        };
        // close can invoke user Wakers. Transfer the exact owner before any
        // caught failure escapes, including a notification after job admission.
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.scope.spawn(Box::pin(async move {
                pending.deactivate().await;
            }))
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
                // Preserve the guest failure already unwinding through this Drop.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(first)));
            } else {
                std::panic::resume_unwind(first);
            }
        }
    }
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::{
        Abis, ExecError, Wasixv2,
        process::tests::{NoThreads, Recorder},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wasixv2_syscalls::TaskId;
    use wasmer::{Function, FunctionEnvMut, Store};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Failure {
        None,
        Prepare,
        Configure,
        Replace,
    }

    struct ProbeHook {
        next: Arc<AtomicUsize>,
        events: Arc<Mutex<Vec<(usize, &'static str)>>>,
        failure: Failure,
        define: bool,
    }

    struct State {
        id: usize,
        configured: bool,
    }
    struct Attachment {
        id: usize,
        env: FunctionEnv<State>,
        events: Arc<Mutex<Vec<(usize, &'static str)>>>,
        failure: Failure,
    }

    impl InstantiationHook<Recorder> for ProbeHook {
        fn prepare_imports(
            &self,
            _: &Module,
            store: &mut StoreMut<'_>,
            _: &FunctionEnv<ExecEnv<Recorder>>,
            imports: &mut Imports,
        ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
            if self.failure == Failure::Prepare {
                return Err(RuntimeError::new("prepare proof"));
            }
            let id = self.next.fetch_add(1, Ordering::SeqCst);
            self.events.lock().unwrap().push((id, "prepare"));
            let env = FunctionEnv::new(
                store,
                State {
                    id,
                    configured: false,
                },
            );
            if self.define || self.failure == Failure::Replace {
                imports.define(
                    "custom",
                    "value",
                    Function::new_typed_with_env(
                        store,
                        &env,
                        |env: FunctionEnvMut<State>| -> Result<i32, RuntimeError> {
                            if !env.data().configured {
                                return Err(RuntimeError::new("binding is not configured"));
                            }
                            Ok(env.data().id as i32)
                        },
                    ),
                );
            } else {
                assert!(imports.get_export("custom", "value").is_some());
            }
            Ok(Some(Box::new(Attachment {
                id,
                env,
                events: self.events.clone(),
                failure: self.failure,
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
            assert_eq!(instance.exports.get_memory("memory").unwrap(), memory);
            assert_eq!(self.env.as_ref(store).id, self.id);
            self.events.lock().unwrap().push((self.id, "configure"));
            if self.failure == Failure::Configure {
                return Err(RuntimeError::new("configure proof"));
            }
            self.env.as_mut(store).configured = true;
            Ok(())
        }
        fn deactivate(&mut self, store: &mut StoreMut<'_>) {
            assert_eq!(self.env.as_ref(store).id, self.id);
            self.env.as_mut(store).configured = false;
            self.events.lock().unwrap().push((self.id, "deactivate"));
        }
    }

    struct ThreadOnlyHook(Arc<AtomicUsize>);
    impl InstantiationHook<Recorder> for ThreadOnlyHook {
        fn requires_owning_thread(&self) -> bool {
            true
        }
        fn prepare_imports(
            &self,
            _: &Module,
            _: &mut StoreMut<'_>,
            _: &FunctionEnv<ExecEnv<Recorder>>,
            _: &mut Imports,
        ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("rejected thread profiles must not prepare bindings");
        }
    }

    fn thread_registry(prepared: Arc<AtomicUsize>) -> Arc<Abis<Recorder>> {
        let mut abis = Abis::wasixv2();
        abis.push_instantiation_hook(Arc::new(ThreadOnlyHook(prepared)));
        Arc::new(abis)
    }

    #[test]
    fn prebuilt_store_cannot_borrow_an_ambient_thread_profile() {
        let store = Store::default();
        let module = module(&store);
        let prepared = Arc::new(AtomicUsize::new(0));
        let control = Arc::new(Recorder::default());
        let task = TaskId(41);
        let owner = crate::thread::OwningThread::acquire().unwrap();
        let result = futures::executor::block_on(crate::run_with_abis(
            store,
            &module,
            control.clone(),
            task,
            Arc::new(NoThreads),
            thread_registry(prepared.clone()),
        ));
        assert!(matches!(result, Err(ExecError::OwningThreadRequired)));
        assert_eq!(prepared.load(Ordering::SeqCst), 0);
        assert_eq!(
            *control.faulted.lock().unwrap(),
            [(task, wasixv2_abi::Signal::SIGSEGV.0)]
        );
        assert_eq!(*control.released.lock().unwrap(), [task]);
        owner.authorize().unwrap();
    }

    #[test]
    fn failed_thread_factory_releases_the_task_without_preparing_bindings() {
        let store = Store::default();
        let module = module(&store);
        let engine = store.engine().clone();
        drop(store);
        let prepared = Arc::new(AtomicUsize::new(0));
        let control = Arc::new(Recorder::default());
        let task = TaskId(42);
        let result = futures::executor::block_on(crate::run_on_owning_thread(
            engine,
            &module,
            control.clone(),
            task,
            Arc::new(NoThreads),
            thread_registry(prepared.clone()),
        ));
        assert!(matches!(result, Err(ExecError::OwningThreadSpawn(_))));
        assert_eq!(prepared.load(Ordering::SeqCst), 0);
        assert_eq!(
            *control.faulted.lock().unwrap(),
            [(task, wasixv2_abi::Signal::SIGSEGV.0)]
        );
        assert_eq!(*control.released.lock().unwrap(), [task]);
    }

    #[test]
    fn occupied_factory_thread_releases_the_task_without_preparing_bindings() {
        struct OccupiedThread;
        impl wasixv2_host::Host for OccupiedThread {
            fn realtime(&self) -> std::time::Duration {
                std::time::Duration::ZERO
            }
            fn monotonic(&self) -> std::time::Duration {
                std::time::Duration::ZERO
            }
            fn sleep(&self, _: std::time::Duration) -> wasixv2_host::LocalFuture<'_, ()> {
                Box::pin(async {})
            }
            fn random(&self, _: &mut [u8]) {}
            fn executor(&self) -> &dyn wasixv2_host::Executor {
                self
            }
        }
        impl wasixv2_host::Executor for OccupiedThread {
            fn spawn_local(&self, _: wasixv2_host::LocalFuture<'static, ()>) {
                panic!("rejected factory must not start local work");
            }
            fn spawn_blocking(
                &self,
                _: Box<dyn FnOnce() + Send>,
            ) -> wasixv2_host::LocalFuture<'static, ()> {
                panic!("rejected factory must not start blocking work");
            }
            fn spawn_thread(&self, start: wasixv2_host::ThreadStart) -> std::io::Result<()> {
                // A host may select an existing worker. Admission must reject
                // this worker's second root before constructing any Store.
                let mut run = start();
                assert!(
                    run.as_mut()
                        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
                        .is_ready()
                );
                Ok(())
            }
        }
        let store = Store::default();
        let module = module(&store);
        let engine = store.engine().clone();
        drop(store);
        let prepared = Arc::new(AtomicUsize::new(0));
        let control = Arc::new(Recorder::default());
        let task = TaskId(43);
        let owner = crate::thread::OwningThread::acquire().unwrap();
        let result = futures::executor::block_on(crate::run_on_owning_thread(
            engine,
            &module,
            control.clone(),
            task,
            Arc::new(OccupiedThread),
            thread_registry(prepared.clone()),
        ));
        assert!(matches!(result, Err(ExecError::OwningThreadBusy)));
        assert_eq!(prepared.load(Ordering::SeqCst), 0);
        assert_eq!(
            *control.faulted.lock().unwrap(),
            [(task, wasixv2_abi::Signal::SIGSEGV.0)]
        );
        assert_eq!(*control.released.lock().unwrap(), [task]);
        owner.authorize().unwrap();
    }

    fn module(store: &Store) -> Module {
        Module::new(
            store,
            wat::parse_str(
                r#"(module
            (import "custom" "value" (func $value (result i32)))
            (import "wasixv2" "exit_group" (func $exit (param i32)))
            (memory (export "memory") 1)
            (func (export "_start") call $value call $exit))"#,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn registry(
        events: Arc<Mutex<Vec<(usize, &'static str)>>>,
        second: Failure,
    ) -> Arc<Abis<Recorder>> {
        registry_with_abi(events, second, Arc::new(Wasixv2))
    }

    fn registry_with_abi(
        events: Arc<Mutex<Vec<(usize, &'static str)>>>,
        second: Failure,
        abi: Arc<dyn crate::Abi<Recorder>>,
    ) -> Arc<Abis<Recorder>> {
        let mut abis = Abis::new();
        abis.push(abi);
        let next = Arc::new(AtomicUsize::new(11));
        abis.push_instantiation_hook(Arc::new(ProbeHook {
            next: next.clone(),
            events: events.clone(),
            failure: Failure::None,
            define: true,
        }));
        abis.push_instantiation_hook(Arc::new(ProbeHook {
            next,
            events,
            failure: second,
            define: false,
        }));
        Arc::new(abis)
    }

    fn instantiate(
        store: Store,
        module: &Module,
        abis: Arc<Abis<Recorder>>,
    ) -> Result<crate::Ready<Recorder>, ExecError> {
        futures::executor::block_on(crate::instantiate(
            store,
            module,
            Arc::new(Recorder::default()),
            TaskId(1),
            Arc::new(NoThreads),
            abis,
            wasixv2_host::LoaderCleanup::new(Arc::new(NoThreads)),
            None,
        ))
    }

    #[test]
    fn same_module_instances_keep_exact_paired_state_and_earlier_imports() {
        let first_store = Store::default();
        let second_store = Store::new(first_store.engine().clone());
        let module = module(&first_store);
        let events = Arc::new(Mutex::new(Vec::new()));
        let abis = registry(events.clone(), Failure::None);
        let first = instantiate(first_store, &module, abis.clone()).unwrap();
        let second = instantiate(second_store, &module, abis).unwrap();
        assert!(matches!(
            futures::executor::block_on(second.start()),
            crate::Ended::Exited(13)
        ));
        assert!(matches!(
            futures::executor::block_on(first.start()),
            crate::Ended::Exited(11)
        ));
        let retired: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, event)| *event == "deactivate")
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(retired, [14, 13, 12, 11]);
    }

    #[test]
    fn prepare_setup_and_import_replacement_fail_before_entry_and_reverse_cleanup() {
        for (failure, expected) in [
            (Failure::Prepare, vec![11]),
            (Failure::Configure, vec![12, 11]),
            (Failure::Replace, vec![12, 11]),
        ] {
            let store = Store::default();
            let module = module(&store);
            let events = Arc::new(Mutex::new(Vec::new()));
            assert!(matches!(
                instantiate(store, &module, registry(events.clone(), failure)),
                Err(ExecError::CustomBinding(_))
            ));
            let retired: Vec<_> = events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, event)| *event == "deactivate")
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(retired, expected);
        }
    }

    #[test]
    fn wasm_start_imports_reject_unconfigured_instance_state() {
        let store = Store::default();
        let module = Module::new(
            &store,
            wat::parse_str(
                r#"(module
            (import "custom" "value" (func $value (result i32)))
            (import "wasixv2" "exit_group" (func $exit (param i32)))
            (memory (export "memory") 1)
            (func $init call $value drop)
            (start $init)
            (func (export "_start")))"#,
            )
            .unwrap(),
        )
        .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let error = match instantiate(store, &module, registry(events.clone(), Failure::None)) {
            Ok(_) => panic!("start must not use an unconfigured binding"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("binding is not configured"));
        assert_eq!(
            *events.lock().unwrap(),
            [
                (11, "prepare"),
                (12, "prepare"),
                (12, "deactivate"),
                (11, "deactivate")
            ]
        );
    }

    // Same explicit local queue as dylink's lease_cancellation_tests; no detached jobs.
    thread_local! {
        static JOBS: std::cell::RefCell<Vec<wasixv2_host::LocalFuture<'static, ()>>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    struct QueueHost;
    impl wasixv2_host::Host for QueueHost {
        fn realtime(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
        fn monotonic(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
        fn sleep(&self, _: std::time::Duration) -> wasixv2_host::LocalFuture<'_, ()> {
            Box::pin(async {})
        }
        fn random(&self, _: &mut [u8]) {}
        fn executor(&self) -> &dyn wasixv2_host::Executor {
            self
        }
        fn report(&self, _: &str) {}
    }
    impl wasixv2_host::Executor for QueueHost {
        fn spawn_local(&self, future: wasixv2_host::LocalFuture<'static, ()>) {
            JOBS.with(|jobs| jobs.borrow_mut().push(future));
        }
        fn spawn_blocking(
            &self,
            work: Box<dyn FnOnce() + Send>,
        ) -> wasixv2_host::LocalFuture<'static, ()> {
            Box::pin(async move { work() })
        }
    }

    #[test]
    fn owning_thread_reports_runner_panics_only_after_transferred_cleanup_retires() {
        use std::{
            future::Future,
            panic::{AssertUnwindSafe, catch_unwind},
            pin::Pin,
            task::{Context, Poll, Waker},
        };
        struct Guest {
            panic_on_poll: bool,
        }
        impl Future for Guest {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
                assert!(!self.panic_on_poll, "guest poll proof");
                Poll::Ready(())
            }
        }
        impl Drop for Guest {
            fn drop(&mut self) {
                panic!("guest destructor proof");
            }
        }
        for panic_on_poll in [false, true] {
            assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
            let scope = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
            let completed = Arc::new(AtomicUsize::new(0));
            let job_completed = completed.clone();
            let (send, receive) = futures::channel::oneshot::channel::<()>();
            assert!(
                scope
                    .submit(Box::new(move || {
                        Box::pin(async move {
                            receive.await.unwrap();
                            job_completed.fetch_add(1, Ordering::SeqCst);
                        })
                    }))
                    .is_ok()
            );
            let profile = crate::thread::OwningThread::acquire().unwrap();
            let mut run = Box::pin(profile.run(&scope, Guest { panic_on_poll }));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert!(matches!(
                crate::thread::OwningThread::acquire(),
                Err(ExecError::OwningThreadBusy)
            ));
            assert!(scope.is_open());
            assert_eq!(completed.load(Ordering::SeqCst), 0);
            let mut jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
            assert_eq!(jobs.len(), 2, "exact cleanup and transferred drain");
            for job in &mut jobs {
                assert!(job.as_mut().poll(&mut cx).is_pending());
            }
            send.send(()).unwrap();
            for job in jobs {
                futures::executor::block_on(job);
            }
            assert_eq!(completed.load(Ordering::SeqCst), 1);
            assert!(!scope.is_open());
            let panic = catch_unwind(AssertUnwindSafe(|| run.as_mut().poll(&mut cx)))
                .expect_err("report the original panic after retirement");
            let expected = if panic_on_poll {
                "guest poll proof"
            } else {
                "guest destructor proof"
            };
            assert_eq!(panic.downcast_ref::<&str>(), Some(&expected));
            drop(run);
            crate::thread::OwningThread::acquire()
                .unwrap()
                .authorize()
                .unwrap();
        }
    }

    #[test]
    fn canceled_deactivation_wait_keeps_exact_store_and_reverse_cleanup_in_runner_custody() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        let store = Store::default();
        let module = module(&store);
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ready =
            instantiate(store, &module, registry(events.clone(), Failure::None)).unwrap();
        let hooks = ready
            .env
            .as_ref(ready.store.as_ref().unwrap())
            .instance_hooks()
            .unwrap()
            .clone();
        let store = ready.store.take().unwrap().into_async();
        let scope = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
        let mut guard = hooks.guard(&store, scope.clone());
        let locked = store.store().write();
        let mut deactivate = Box::pin(guard.deactivate());
        assert!(
            deactivate
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(deactivate);
        drop(guard);
        assert!(
            !events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, event)| *event == "deactivate")
        );
        drop(locked);
        let jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
        assert_eq!(
            jobs.len(),
            1,
            "cancellation must transfer one owned continuation"
        );
        for job in jobs {
            futures::executor::block_on(job);
        }
        assert_eq!(futures::executor::block_on(scope.run(async { 41 })), 41);
        assert!(!scope.is_open());
        let retired: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, event)| *event == "deactivate")
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(retired, [12, 11]);
        let mut locked = store.store().write();
        hooks.deactivate(&mut locked.as_store_mut());
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, event)| *event == "deactivate")
                .count(),
            2
        );
    }

    #[test]
    fn dropping_a_ready_instance_deactivates_on_its_actual_store_without_entry() {
        let store = Store::default();
        let module = module(&store);
        let events = Arc::new(Mutex::new(Vec::new()));
        let ready = instantiate(store, &module, registry(events.clone(), Failure::None)).unwrap();
        drop(ready);
        let retired: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, event)| *event == "deactivate")
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(retired, [12, 11]);
    }
    // Match custom_bindings' bounded child ownership: a broken settlement fence
    // must fail the test rather than hang this test process during stack teardown.
    fn native_scope_subprocess(name: &str) -> bool {
        const CHILD: &str = "WASIXV2_NATIVE_SCOPE_CHILD";
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
                    assert!(status.success(), "native cleanup proof failed: {status}");
                    return false;
                }
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => panic!("native cleanup proof did not finish: {result:?}"),
            }
        }
    }

    type RetainedCall = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Box<[wasmer::Value]>, RuntimeError>>>,
    >;
    thread_local! {
        static NATIVE_WAKER: std::cell::RefCell<Option<std::task::Waker>> = const { std::cell::RefCell::new(None) };
        static NATIVE_CALL: std::cell::RefCell<Option<RetainedCall>> = const { std::cell::RefCell::new(None) };
        static NATIVE_ROOT: std::cell::RefCell<Option<wasmer::sys::WeakNativeInvocation>> = const { std::cell::RefCell::new(None) };
        static NATIVE_FRAME: std::cell::RefCell<Option<wasmer::sys::NativeFrameGuard>> = const { std::cell::RefCell::new(None) };
        static NATIVE_WEAK: std::cell::RefCell<Option<wasmer::WeakAsyncFunctionEnvMut<ExecEnv<Recorder>>>> = const { std::cell::RefCell::new(None) };
        static NATIVE_OUTCOME: std::cell::RefCell<Option<Result<(), RuntimeError>>> = const { std::cell::RefCell::new(None) };
        static NATIVE_STACK_ERROR: std::cell::RefCell<Option<RuntimeError>> = const { std::cell::RefCell::new(None) };
        static NATIVE_OBSERVED_STORE: std::cell::RefCell<Option<StoreAsync>> = const { std::cell::RefCell::new(None) };
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum RetainedPhase {
        StartError,
        StartPending,
        StartPanic,
        StartOutcome,
        PreparationError,
    }

    async fn retain_pending_invocation(env: FunctionEnv<ExecEnv<Recorder>>, store: StoreAsync) {
        use std::task::{Context, Waker};
        let function = {
            let mut store_lock = store.write_lock().await;
            let stacked = NATIVE_STACK_ERROR.with(|slot| slot.borrow().is_some());
            if stacked {
                NATIVE_OBSERVED_STORE.with(|slot| *slot.borrow_mut() = Some(store.store()));
            }
            let store = &mut store_lock.as_store_mut();
            if stacked {
                let owner = env.as_ref(&*store).contexts.clone();
                Function::new_typed(store, move || -> Result<i32, RuntimeError> {
                    let context = owner.upgrade().unwrap().current();
                    let error = NATIVE_STACK_ERROR.with(|slot| slot.borrow_mut().take().unwrap());
                    let custody = crate::contexts::PendingError::new(
                        Some((owner.clone(), context)),
                        Some(&error),
                    );
                    drop(error);
                    // Match API cross_store_proof: the foreign synchronous suffix
                    // retains this physical frame while releasing A's actual lock.
                    let mut foreign = Store::default();
                    let pending = Function::new_typed_async(&mut foreign, || {
                        NATIVE_ROOT.with(|slot| {
                            *slot.borrow_mut() = wasmer::sys::WeakNativeInvocation::current()
                        });
                        std::future::pending::<Result<i32, RuntimeError>>()
                    });
                    let module = Module::new(
                        &foreign,
                        r#"(module
                        (import "test" "pending" (func $pending (result i32)))
                        (func (export "run") (result i32) (call $pending)))"#,
                    )
                    .unwrap();
                    let instance = Instance::new(
                        &mut foreign,
                        &module,
                        &wasmer::imports! { "test" => { "pending" => pending } },
                    )
                    .unwrap();
                    let result = instance
                        .exports
                        .get_typed_function::<(), i32>(&foreign, "run")
                        .unwrap()
                        .call(&mut foreign);
                    drop(custody);
                    result
                })
            } else {
                Function::new_typed_with_env_async(
                    store,
                    &env,
                    |env: wasmer::AsyncFunctionEnvMut<ExecEnv<Recorder>>| {
                        NATIVE_ROOT.with(|slot| {
                            *slot.borrow_mut() = wasmer::sys::WeakNativeInvocation::current()
                        });
                        NATIVE_WEAK.with(|slot| *slot.borrow_mut() = env.downgrade());
                        std::future::pending::<Result<i32, RuntimeError>>()
                    },
                )
            }
        };
        let mut call: RetainedCall =
            Box::pin(async move { function.call_async(&store, vec![]).await });
        let waker = NATIVE_WAKER
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(|| Waker::noop().clone());
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let root = NATIVE_ROOT.with(|slot| slot.borrow_mut().take().unwrap());
        // Model a caller-held external native scope. Until it exits, this
        // test never independently parks another physical native root.
        let frame = unsafe { root.enter_descendant_frame() }.unwrap();
        NATIVE_FRAME.with(|slot| *slot.borrow_mut() = Some(frame));
        NATIVE_CALL.with(|slot| *slot.borrow_mut() = Some(call));
    }

    // An ABI can fail while an externally owned invocation still retains its
    // physical stack. This owner is deliberately outside Store/attachment state.
    struct RetainedInvocation(Arc<dyn crate::AbiInstance<Recorder>>, RetainedPhase);
    impl crate::AbiInstance<Recorder> for RetainedInvocation {
        fn start<'a>(
            &'a self,
            entry: &'a crate::Entry<'a, Recorder>,
        ) -> wasixv2_syscalls::LocalFuture<'a, Result<(), RuntimeError>> {
            Box::pin(async move {
                retain_pending_invocation(entry.cx().env.clone(), entry.store()).await;
                if self.1 == RetainedPhase::StartOutcome {
                    return NATIVE_OUTCOME.with(|slot| slot.borrow_mut().take().unwrap());
                }
                if self.1 == RetainedPhase::StartPending {
                    return std::future::pending().await;
                }
                assert!(
                    self.1 != RetainedPhase::StartPanic,
                    "retained native guest panic"
                );
                Err(RuntimeError::new(
                    "ABI failed with a retained native invocation",
                ))
            })
        }
        fn on_entry_return(&self) -> crate::EntryReturn {
            self.0.on_entry_return()
        }
        fn start_thread<'a>(
            &'a self,
            entry: &'a crate::Entry<'a, Recorder>,
            tid: i32,
            arg: u32,
        ) -> wasixv2_syscalls::LocalFuture<'a, Result<(), RuntimeError>> {
            self.0.start_thread(entry, tid, arg)
        }
        fn on_thread_return(&self) -> crate::EntryReturn {
            self.0.on_thread_return()
        }
        fn deliver_signal<'a>(
            &'a self,
            env: &'a wasmer::AsyncFunctionEnvMut<ExecEnv<Recorder>>,
            frame: &'a wasixv2_syscalls::SignalFrame,
            restart: bool,
        ) -> wasixv2_syscalls::LocalFuture<'a, Result<crate::Sigreturn, RuntimeError>> {
            self.0.deliver_signal(env, frame, restart)
        }
        fn vfork_resume<'a>(
            &'a self,
            env: &'a wasmer::AsyncFunctionEnvMut<ExecEnv<Recorder>>,
            child: TaskId,
            pid: i32,
        ) -> wasixv2_syscalls::LocalFuture<'a, crate::VforkResume> {
            self.0.vfork_resume(env, child, pid)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn context_outcomes_retain_owned_exit_and_exec_downcasts_after_physical_settlement() {
        if !native_scope_subprocess(
            "instantiation::tests::context_outcomes_retain_owned_exit_and_exec_downcasts_after_physical_settlement",
        ) {
            return;
        }
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        for exec in [false, true] {
            assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
            let store = Store::default();
            let module = module(&store);
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut ready =
                instantiate(store, &module, registry(events.clone(), Failure::None)).unwrap();
            ready.abi = Arc::new(RetainedInvocation(
                ready.abi.clone(),
                RetainedPhase::StartOutcome,
            ));
            let error = if exec {
                RuntimeError::user(Box::new(crate::process::ExecNew(
                    wasixv2_syscalls::Image::copied(vec![17, 23]),
                )))
            } else {
                RuntimeError::user(Box::new(crate::Exited(37)))
            };
            NATIVE_OUTCOME.with(|slot| *slot.borrow_mut() = Some(Err(error)));
            let mut run = Box::pin(ready.start());
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            drop(NATIVE_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
            match (exec, futures::executor::block_on(run)) {
                (false, crate::Ended::Exited(37)) => (),
                (true, crate::Ended::Exec(image)) => assert_eq!(image.bytes(), [17, 23]),
                _ => panic!("physical settlement must release every error transport clone"),
            }
            NATIVE_WEAK.with(|slot| drop(slot.borrow_mut().take()));
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("original retired context error")]
    struct ContextPayload(Arc<AtomicUsize>);
    impl Drop for ContextPayload {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn context_error_custody_outlives_canceled_runner_and_escaped_native_frame() {
        if !native_scope_subprocess(
            "instantiation::tests::context_error_custody_outlives_canceled_runner_and_escaped_native_frame",
        ) {
            return;
        }
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        for cancel in [false, true] {
            assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
            let store = Store::default();
            let module = module(&store);
            let events = Arc::new(Mutex::new(Vec::new()));
            let cleanup = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
            let mut ready = futures::executor::block_on(crate::instantiate(
                store,
                &module,
                Arc::new(Recorder::default()),
                TaskId(1),
                Arc::new(QueueHost),
                registry(events, Failure::None),
                cleanup,
                None,
            ))
            .unwrap();
            ready.abi = Arc::new(RetainedInvocation(
                ready.abi.clone(),
                RetainedPhase::StartOutcome,
            ));
            let dropped = Arc::new(AtomicUsize::new(0));
            let error = RuntimeError::user(Box::new(ContextPayload(dropped.clone())));
            if cancel {
                NATIVE_OUTCOME.with(|slot| *slot.borrow_mut() = Some(Err(error)));
            } else {
                NATIVE_STACK_ERROR.with(|slot| *slot.borrow_mut() = Some(error));
                NATIVE_OUTCOME.with(|slot| *slot.borrow_mut() = Some(Ok(())));
            }
            let mut run = Box::pin(ready.start());
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            drop(NATIVE_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert_eq!(dropped.load(Ordering::SeqCst), 0);
            if cancel {
                drop(run);
                assert_eq!(
                    dropped.load(Ordering::SeqCst),
                    0,
                    "the attachment retains terminal custody"
                );
                let mut jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
                assert_eq!(jobs.len(), 1);
                for job in &mut jobs {
                    assert!(job.as_mut().poll(&mut cx).is_pending());
                }
                assert_eq!(
                    dropped.load(Ordering::SeqCst),
                    0,
                    "the escaped frame still owns settlement"
                );
                drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
                for job in jobs {
                    futures::executor::block_on(job);
                }
            } else {
                let observed = NATIVE_OBSERVED_STORE.with(|slot| slot.borrow_mut().take().unwrap());
                let mut lock = Box::pin(observed.write_lock());
                let writable = match lock.as_mut().poll(&mut cx) {
                    std::task::Poll::Ready(guard) => {
                        drop(guard);
                        true
                    }
                    std::task::Poll::Pending => false,
                };
                drop(lock);
                drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
                let ended = futures::executor::block_on(run);
                assert!(
                    writable,
                    "A's Store lock must be free while its physical frame remains pinned"
                );
                let crate::Ended::Fault { report, .. } = ended else {
                    panic!("late original error must replace successful logical return");
                };
                assert!(report.contains("original retired context error"));
            }
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
            NATIVE_WEAK.with(|slot| drop(slot.borrow_mut().take()));
        }
    }

    #[test]
    fn native_invocations_settle_before_task_fault_and_attachment_deactivation() {
        if !native_scope_subprocess(
            "instantiation::tests::native_invocations_settle_before_task_fault_and_attachment_deactivation",
        ) {
            return;
        }
        use std::{
            future::Future,
            task::{Context, Wake, Waker},
        };
        struct PendingWaiter {
            dropped: Arc<AtomicUsize>,
            woke: Arc<AtomicUsize>,
        }
        impl Wake for PendingWaiter {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.woke.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Drop for PendingWaiter {
            fn drop(&mut self) {
                self.dropped.fetch_add(1, Ordering::SeqCst);
                panic!("cleanup waiter drop");
            }
        }
        for cancel in [false, true] {
            assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
            let store = Store::default();
            let module = module(&store);
            let recorder = Arc::new(Recorder::default());
            let events = Arc::new(Mutex::new(Vec::new()));
            let cleanup = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
            let mut ready = futures::executor::block_on(crate::instantiate(
                store,
                &module,
                recorder.clone(),
                TaskId(1),
                Arc::new(QueueHost),
                registry(events.clone(), Failure::None),
                cleanup.clone(),
                None,
            ))
            .unwrap();
            // Every native runner already owns its invocation generation.
            assert!(ready.invocations.is_some());
            ready.abi = Arc::new(RetainedInvocation(
                ready.abi.clone(),
                if cancel {
                    RetainedPhase::StartPending
                } else {
                    RetainedPhase::StartError
                },
            ));
            let profile = crate::thread::OwningThread::acquire().unwrap();
            let mut run = Box::pin(profile.run(&cleanup, ready.start()));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(run.as_mut().poll(&mut cx).is_pending());
            assert!(
                recorder.faulted.lock().unwrap().is_empty(),
                "ended must wait for physical settlement"
            );
            assert!(
                !events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, event)| *event == "deactivate")
            );
            let initially_closed =
                NATIVE_WEAK.with(|slot| slot.borrow().as_ref().unwrap().upgrade().is_none());
            let mut canceled_before_job_poll = true;
            drop(NATIVE_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
            assert!(
                run.as_mut().poll(&mut cx).is_pending(),
                "frame pin still owns the native stack"
            );
            if cancel {
                drop(run);
                canceled_before_job_poll =
                    NATIVE_WEAK.with(|slot| slot.borrow().as_ref().unwrap().upgrade().is_none());
                let mut jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
                assert_eq!(
                    jobs.len(),
                    2,
                    "one attachment cleanup and its transferred runner drain"
                );
                let mut failed = jobs.remove(0);
                let waiter_drops = Arc::new(AtomicUsize::new(0));
                let waiter_wakes = Arc::new(AtomicUsize::new(0));
                let waiter = Waker::from(Arc::new(PendingWaiter {
                    dropped: waiter_drops.clone(),
                    woke: waiter_wakes.clone(),
                }));
                assert!(
                    failed
                        .as_mut()
                        .poll(&mut Context::from_waker(&waiter))
                        .is_pending()
                );
                drop(waiter);
                let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    failed.as_mut().poll(&mut cx)
                }))
                .expect_err("replacing the queued waiter's last Waker invokes its destructor");
                // A panicked task is dropped by its executor. That must transfer
                // the exact native guard again before old Completion retires.
                drop(failed);
                let replacement = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
                let replacement_count = replacement.len();
                for job in replacement.into_iter().rev() {
                    jobs.insert(0, job);
                }
                let mut early_completion = false;
                let mut pending = Vec::new();
                for mut job in jobs {
                    if job.as_mut().poll(&mut cx).is_pending() {
                        pending.push(job);
                    } else {
                        early_completion = true;
                    }
                }
                let busy = matches!(
                    crate::thread::OwningThread::acquire(),
                    Err(ExecError::OwningThreadBusy)
                );
                let early_deactivation = events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, event)| *event == "deactivate");
                drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
                for job in pending {
                    futures::executor::block_on(job);
                }
                assert_eq!(
                    replacement_count, 1,
                    "a failed cleanup poll transfers one exact native continuation"
                );
                assert!(
                    !early_completion,
                    "runner drain still owns pending native cleanup"
                );
                assert!(
                    busy,
                    "thread lease stays registered while its native stack is pinned"
                );
                assert!(!early_deactivation);
                assert_eq!(panic.downcast_ref::<&str>(), Some(&"cleanup waiter drop"));
                assert_eq!(waiter_drops.load(Ordering::SeqCst), 1);
                assert_eq!(waiter_wakes.load(Ordering::SeqCst), 0);
                assert!(recorder.faulted.lock().unwrap().is_empty());
                crate::thread::OwningThread::acquire()
                    .unwrap()
                    .authorize()
                    .unwrap();
            } else {
                drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
                assert!(matches!(
                    futures::executor::block_on(run),
                    crate::Ended::Fault { .. }
                ));
                assert_eq!(recorder.faulted.lock().unwrap().len(), 1);
            }
            assert_eq!(
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, event)| *event == "deactivate")
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                [12, 11]
            );
            NATIVE_WEAK.with(|slot| drop(slot.borrow_mut().take()));
            assert_eq!(
                initially_closed, !cancel,
                "normal completion closes native callback admission before ended"
            );
            assert!(
                canceled_before_job_poll,
                "cancellation closes callback admission before the cleanup job is polled"
            );
            futures::executor::block_on(cleanup.run(async {}));
            assert!(!cleanup.is_open());
        }
    }
    struct ScopeOnlyHook;
    impl InstantiationHook<Recorder> for ScopeOnlyHook {
        fn requires_owning_thread(&self) -> bool {
            true
        }
        fn prepare_imports(
            &self,
            _: &Module,
            _: &mut StoreMut<'_>,
            _: &FunctionEnv<ExecEnv<Recorder>>,
            _: &mut Imports,
        ) -> Result<Option<Box<dyn InstanceAttachment>>, RuntimeError> {
            Ok(None)
        }
    }

    #[test]
    fn preparation_transfers_scope_without_attachments_on_cancel_and_busy_consume() {
        use crate::preparation::tests::{Mode, setup, take_retained_store};
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        for mode in [Mode::Pending, Mode::RetainedStore] {
            assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
            let (store, module, recorder, probe, mut abis) = setup(mode);
            Arc::get_mut(&mut abis)
                .unwrap()
                .push_instantiation_hook(Arc::new(ScopeOnlyHook));
            let cleanup = wasixv2_host::LoaderCleanup::new(Arc::new(QueueHost));
            let mut instantiate = Box::pin(crate::instantiate(
                store,
                &module,
                recorder.clone(),
                TaskId(1),
                Arc::new(QueueHost),
                abis,
                cleanup.clone(),
                None,
            ));
            if mode == Mode::Pending {
                assert!(
                    instantiate
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
                drop(instantiate);
                let jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
                assert_eq!(
                    jobs.len(),
                    1,
                    "scope-only preparation still owns a cleanup continuation"
                );
                for job in jobs {
                    futures::executor::block_on(job);
                }
            } else {
                let error = match futures::executor::block_on(instantiate) {
                    Err(error) => error,
                    Ok(_) => {
                        panic!("retained preparation Store must prevent exclusive consumption")
                    }
                };
                assert!(matches!(
                    error,
                    ExecError::Load(crate::LoadError::Format(wasixv2_syscalls::Errno::EBUSY))
                ));
                let retained = take_retained_store();
                let mut write = retained.store().write();
                let fresh = Function::new_typed(&mut write.as_store_mut(), || -> i32 { 42 });
                assert!(
                    fresh
                        .call(&mut write.as_store_mut(), &[])
                        .unwrap_err()
                        .is_host_abort(),
                    "the exact failed preparation generation remains closed"
                );
                drop(write);
                drop(retained);
                assert!(
                    JOBS.with(|jobs| jobs.borrow().is_empty()),
                    "busy cleanup completed before returning the owned error"
                );
            }
            assert_eq!(probe.guest_calls.load(Ordering::Relaxed), 0);
            assert!(!recorder.loader.as_ref().unwrap().busy());
            futures::executor::block_on(cleanup.run(async {}));
            assert!(!cleanup.is_open());
        }
    }
    struct RetainedAbi(RetainedPhase);
    impl crate::Abi<Recorder> for RetainedAbi {
        fn prepare<'a>(
            &'a self,
            cx: &'a crate::Preparation<'a, Recorder>,
        ) -> wasixv2_syscalls::LocalFuture<
            'a,
            Result<Option<Box<dyn crate::PreparedImage<Recorder>>>, ExecError>,
        > {
            Box::pin(async move {
                if self.0 == RetainedPhase::PreparationError {
                    // This private test seam deliberately exercises a backend's
                    // retained-root error. Public preparation remains host-only.
                    let store = crate::preparation::tests::store_for_retirement_proof(cx);
                    retain_pending_invocation(cx.environment().clone(), store).await;
                    return Err(ExecError::Load(crate::LoadError::Format(
                        wasixv2_syscalls::Errno::EINVAL,
                    )));
                }
                Ok(None)
            })
        }
        fn name(&self) -> &'static str {
            "native-retirement-panic-proof"
        }
        fn matches(&self, module: &Module) -> bool {
            <Wasixv2 as crate::Abi<Recorder>>::matches(&Wasixv2, module)
        }
        fn matches_image(&self, image: &[u8]) -> bool {
            <Wasixv2 as crate::Abi<Recorder>>::matches_image(&Wasixv2, image)
        }
        fn imports(
            &self,
            store: &mut Store,
            env: &FunctionEnv<ExecEnv<Recorder>>,
            module: &Module,
        ) -> Imports {
            <Wasixv2 as crate::Abi<Recorder>>::imports(&Wasixv2, store, env, module)
        }
        fn bind(
            &self,
            instance: &Instance,
            store: &mut Store,
            task: TaskId,
            role: crate::Role,
        ) -> Result<Arc<dyn crate::AbiInstance<Recorder>>, ExecError> {
            let inner =
                <Wasixv2 as crate::Abi<Recorder>>::bind(&Wasixv2, instance, store, task, role)?;
            Ok(Arc::new(RetainedInvocation(inner, self.0)))
        }
        fn low_guard(&self) -> bool {
            <Wasixv2 as crate::Abi<Recorder>>::low_guard(&Wasixv2)
        }
    }

    #[test]
    fn profiled_panic_reports_to_kernel_only_after_native_cleanup_and_lease_retirement() {
        if !native_scope_subprocess(
            "instantiation::tests::profiled_panic_reports_to_kernel_only_after_native_cleanup_and_lease_retirement",
        ) {
            return;
        }
        use std::{
            future::Future,
            task::{Context, Wake, Waker},
        };
        struct ClosePanic(Arc<AtomicUsize>);
        impl Wake for ClosePanic {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("secondary scope notification panic");
            }
        }
        assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
        let profile = crate::thread::OwningThread::acquire().unwrap();
        let store = Store::default();
        let module = module(&store);
        let recorder = Arc::new(Recorder::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut abis = registry_with_abi(
            events.clone(),
            Failure::None,
            Arc::new(RetainedAbi(RetainedPhase::StartPanic)),
        );
        Arc::get_mut(&mut abis)
            .unwrap()
            .push_instantiation_hook(Arc::new(ScopeOnlyHook));
        let wakes = Arc::new(AtomicUsize::new(0));
        NATIVE_WAKER.with(|slot| {
            *slot.borrow_mut() = Some(Waker::from(Arc::new(ClosePanic(wakes.clone()))))
        });
        let mut run = Box::pin(crate::run_with_abis_profiled(
            store,
            &module,
            recorder.clone(),
            TaskId(1),
            Arc::new(QueueHost),
            abis,
            Some(profile),
        ));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        let early_faults = recorder.faulted.lock().unwrap().len();
        let early_releases = recorder.released.lock().unwrap().len();
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            1,
            "Drop closes admission even while the original guest panic unwinds"
        );
        NATIVE_WEAK.with(|slot| assert!(slot.borrow().as_ref().unwrap().upgrade().is_none()));
        let mut jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
        assert_eq!(jobs.len(), 2);
        for job in &mut jobs {
            assert!(job.as_mut().poll(&mut cx).is_pending());
        }
        assert!(
            !events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, event)| *event == "deactivate")
        );
        assert!(matches!(
            crate::thread::OwningThread::acquire(),
            Err(ExecError::OwningThreadBusy)
        ));
        drop(NATIVE_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
        drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
        for job in jobs {
            futures::executor::block_on(job);
        }
        let error = futures::executor::block_on(run).unwrap_err();
        assert!(
            matches!(error, ExecError::Panic(message) if message == "retained native guest panic")
        );
        assert_eq!(recorder.faulted.lock().unwrap().len(), 1);
        assert_eq!(*recorder.released.lock().unwrap(), [TaskId(1)]);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, event)| *event == "deactivate")
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [12, 11]
        );
        NATIVE_WEAK.with(|slot| drop(slot.borrow_mut().take()));
        crate::thread::OwningThread::acquire()
            .unwrap()
            .authorize()
            .unwrap();
        // Capture before cleanup, assert afterward so a regression still retires
        // every owned test stack before reporting this controlled failure.
        assert_eq!(
            early_faults, 0,
            "kernel fault must follow actual native retirement"
        );
        assert_eq!(
            early_releases, 0,
            "task release must follow actual native retirement"
        );
    }
    #[test]
    fn ordinary_preparation_error_waits_for_its_native_invocation_before_task_release() {
        if !native_scope_subprocess(
            "instantiation::tests::ordinary_preparation_error_waits_for_its_native_invocation_before_task_release",
        ) {
            return;
        }
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        assert!(JOBS.with(|jobs| jobs.borrow().is_empty()));
        let profile = crate::thread::OwningThread::acquire().unwrap();
        let store = Store::default();
        let module = module(&store);
        let recorder = Arc::new(Recorder::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut abis = registry_with_abi(
            events.clone(),
            Failure::None,
            Arc::new(RetainedAbi(RetainedPhase::PreparationError)),
        );
        Arc::get_mut(&mut abis)
            .unwrap()
            .push_instantiation_hook(Arc::new(ScopeOnlyHook));
        let mut run = Box::pin(crate::run_with_abis_profiled(
            store,
            &module,
            recorder.clone(),
            TaskId(1),
            Arc::new(QueueHost),
            abis,
            Some(profile),
        ));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        let early_faults = recorder.faulted.lock().unwrap().len();
        let early_releases = recorder.released.lock().unwrap().len();
        NATIVE_WEAK.with(|slot| assert!(slot.borrow().as_ref().unwrap().upgrade().is_none()));
        assert!(
            events.lock().unwrap().is_empty(),
            "instance hooks have not run during ABI preparation"
        );
        assert!(matches!(
            crate::thread::OwningThread::acquire(),
            Err(ExecError::OwningThreadBusy)
        ));
        drop(NATIVE_CALL.with(|slot| slot.borrow_mut().take().unwrap()));
        drop(NATIVE_FRAME.with(|slot| slot.borrow_mut().take().unwrap()));
        let mut result = None;
        for _ in 0..4 {
            let polled = run.as_mut().poll(&mut cx);
            let jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
            for job in jobs {
                futures::executor::block_on(job);
            }
            if let Poll::Ready(value) = polled {
                result = Some(value);
                break;
            }
        }
        assert!(matches!(
            result.expect("bounded cleanup completes after final pin release"),
            Err(ExecError::Load(crate::LoadError::Format(
                wasixv2_syscalls::Errno::EINVAL
            )))
        ));
        drop(run);
        assert_eq!(recorder.faulted.lock().unwrap().len(), 1);
        assert_eq!(*recorder.released.lock().unwrap(), [TaskId(1)]);
        NATIVE_WEAK.with(|slot| drop(slot.borrow_mut().take()));
        crate::thread::OwningThread::acquire()
            .unwrap()
            .authorize()
            .unwrap();
        assert_eq!(
            early_faults, 0,
            "ordinary setup errors must wait for physical retirement"
        );
        assert_eq!(
            early_releases, 0,
            "ordinary setup errors must retain the task until then"
        );
    }
}
