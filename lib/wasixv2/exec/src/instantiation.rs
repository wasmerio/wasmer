//! Ordered, store-local custom bindings, independent of the selected process ABI.

use std::sync::{Arc, Mutex};
use wasmer::{
    AsStoreAsync, AsStoreMut, FunctionEnv, Imports, Instance, Memory, Module, RuntimeError,
    StoreAsync, StoreMut,
};

use crate::ExecEnv;

/// Adds imports after ABI preparation. Existing imports must remain unchanged.
/// Each successful preparation returns state belonging to this exact instance and Store.
/// An engine-level Wasm `start` can call imports before configuration: imports that
/// need the bound instance must reject calls until their attachment is configured.
pub trait InstantiationHook<S>: Send + Sync + 'static {
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

    pub(crate) fn guard(
        &self,
        store: &StoreAsync,
        scope: Arc<wasixv2_host::LoaderCleanup>,
    ) -> AttachmentGuard {
        AttachmentGuard {
            hooks: Some(self.clone()),
            store: store.store(),
            scope,
        }
    }
}

/// Cancellation transfers the actual state and Store to the runner's cleanup queue.
pub(crate) struct AttachmentGuard {
    hooks: Option<InstanceHooks>,
    store: StoreAsync,
    scope: Arc<wasixv2_host::LoaderCleanup>,
}

impl AttachmentGuard {
    pub(crate) async fn deactivate(&mut self) {
        let mut store = self.store.write_lock().await;
        if let Some(hooks) = self.hooks.take() {
            hooks.deactivate(&mut store.as_store_mut());
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.hooks = None;
    }
}

impl Drop for AttachmentGuard {
    fn drop(&mut self) {
        if let Some(hooks) = self.hooks.take() {
            if hooks
                .attached
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cells
                .iter()
                .all(|cell| cell.lock().unwrap_or_else(|e| e.into_inner()).is_none())
            {
                return;
            }
            let store = self.store.store();
            self.scope.spawn(Box::pin(async move {
                let mut store = store.write_lock().await;
                hooks.deactivate(&mut store.as_store_mut());
            }));
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
        let mut abis = Abis::new();
        abis.push(Arc::new(Wasixv2));
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
}
