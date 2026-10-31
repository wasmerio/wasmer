//! Loader-owned prepared DSO code, shared across compatible stores and JS realms.

use crate::Initialization;
use std::sync::Arc;
use wasmer::{AsEngineRef, Module, RuntimeError};

fn error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(error.to_string())
}

/// The constructor couples one validated initialization plan to the code compiled
/// from that same plan. Only instance-specific imports/effects remain for staging.
/// Like v1's DlModule, this owns compiled code rather than recompiling per instance.
pub(crate) struct PreparedArtifact {
    initialization: Arc<Initialization>,
    module: Module,
}

impl PreparedArtifact {
    pub(crate) fn compile(
        engine: &impl AsEngineRef,
        initialization: Arc<Initialization>,
    ) -> Result<Self, RuntimeError> {
        for payload in wasmparser::Parser::new(0).parse_all(initialization.original_source()) {
            if let wasmparser::Payload::TableSection(section) = payload.map_err(error)?
                && section.count() > 0
            {
                return Err(error("side module defines a private function table"));
            }
        }
        let bytes = initialization.instantiate_bytes().map_err(error)?;
        // Do not inherit load()'s unpreemptible fallback. Every side-module loop
        // reaches the ordinary return-to-guest boundary on every store/realm.
        let bytes = wasixv2_exec::instrument(&bytes).map_err(error)?;
        let module = Module::new(engine, bytes).map_err(error)?;
        Ok(Self {
            initialization,
            module,
        })
    }

    pub(crate) fn initialization(&self) -> &Initialization {
        &self.initialization
    }

    pub(crate) fn module(&self) -> &Module {
        &self.module
    }
}

#[cfg(all(
    test,
    any(feature = "engine-tests", all(feature = "js", target_arch = "wasm32"))
))]
mod test_fixture {
    use crate::Initialization;
    use std::sync::Arc;
    use wasmer::{
        Engine, Function, Global, Imports, Memory, MemoryType, Store, Table, TableType, Type, Value,
    };

    pub(super) fn initialization() -> Arc<Initialization> {
        Arc::new(
            Initialization::parse(&crate::tests::bytes(
                r#"(module
                (import "env" "memory" (memory 1 4 shared))
                (import "env" "__indirect_function_table" (table 3 funcref))
                (import "env" "__memory_base" (global $base i32))
                (import "env" "__table_base" (global $table i32))
                (global $state (mut i32) (i32.const 0))
                (func $f (export "value") (result i32) global.get $base)
                (func $start i32.const 7 global.set $state)
                (func (export "__wasm_init_tls") (param i32)
                    local.get 0 i32.const 11 i32.store)
                (elem (global.get $table) func $f)
                (data (global.get $base) "new")
                (start $start))"#,
                [16, 0, 1, 0],
                &[],
                &[],
                &[],
            ))
            .unwrap(),
        )
    }
    pub(super) fn fixture(
        engine: &Engine,
        base: i32,
        slot: i32,
    ) -> (Store, Memory, Table, Imports) {
        let mut store = Store::new(engine.clone());
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
        let table = Table::new(
            &mut store,
            TableType::new(Type::FuncRef, 3, Some(64)),
            Value::FuncRef(None),
        )
        .unwrap();
        let mut imports = Imports::new();
        imports.define("env", "memory", memory.clone());
        imports.define("env", "__indirect_function_table", table.clone());
        imports.define(
            "env",
            "__memory_base",
            Global::new(&mut store, Value::I32(base)),
        );
        imports.define(
            "env",
            "__table_base",
            Global::new(&mut store, Value::I32(slot)),
        );
        // Same typed hooks as the existing native staging sibling. Preparation and
        // host Apply must not execute any helper, even one that reaches this hook.
        imports.define(
            "wasixv2-exec",
            "preempt",
            Function::new_typed(&mut store, || 65536i32),
        );
        imports.define(
            "wasixv2-exec",
            "unreachable",
            Function::new_typed(&mut store, || -> i32 { panic!("unexpected guest trap") }),
        );
        imports.define(
            "wasixv2-exec",
            "memory-grow",
            Function::new_typed(&mut store, |_: i32| -1i32),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-wait32",
            Function::new_typed(&mut store, |_: i32, _: i32, _: i64, _: i32| -> i32 {
                panic!("unexpected wait32")
            }),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-wait64",
            Function::new_typed(&mut store, |_: i32, _: i64, _: i64, _: i32| -> i32 {
                panic!("unexpected wait64")
            }),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-notify",
            Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 {
                panic!("unexpected notify")
            }),
        );
        (store, memory, table, imports)
    }
}

#[cfg(all(test, feature = "engine-tests"))]
mod tests {
    use super::test_fixture::{fixture, initialization};
    use super::*;
    use crate::store::{BoundModule, StagedExports};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wasmer::{
        Engine, Memory, MemoryType, Value,
        sys::{CompilerConfig, Cranelift, EngineBuilder, FunctionMiddleware, ModuleMiddleware},
    };
    use wasmer_types::{LocalFunctionIndex, MiddlewareError, ModuleInfo};

    #[derive(Debug)]
    struct CountModules(Arc<AtomicUsize>);
    #[derive(Debug)]
    struct Pass;
    impl<'a> FunctionMiddleware<'a> for Pass {}
    impl ModuleMiddleware for CountModules {
        fn generate_function_middleware<'a>(
            &self,
            _: LocalFunctionIndex,
        ) -> Box<dyn FunctionMiddleware<'a> + 'a> {
            Box::new(Pass)
        }
        fn transform_module_info(&self, _: &mut ModuleInfo) -> Result<(), MiddlewareError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    fn engine() -> (Engine, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let mut compiler = Cranelift::default();
        compiler.push_middleware(Arc::new(CountModules(count.clone())));
        (EngineBuilder::new(compiler).engine().into(), count)
    }
    #[test]
    fn one_compilation_replays_store_specific_effects_and_retains_code_after_artifact_drop() {
        fn send_sync<T: Send + Sync>() {}
        fn send<T: Send>() {}
        send_sync::<BoundModule>();
        send::<StagedExports>();
        let (engine, compilations) = engine();
        let plan = initialization();
        let artifact = Arc::new(PreparedArtifact::compile(&engine, plan.clone()).unwrap());
        assert!(std::ptr::eq(artifact.initialization(), &*plan));
        assert_eq!(compilations.load(Ordering::Relaxed), 1);
        let weak = Arc::downgrade(&artifact);
        let mut published = Vec::new();
        for (base, slot, tls) in [(128, 3, None), (256, 5, None), (384, 7, Some(512))] {
            let (mut store, memory, table, imports) = fixture(&engine, base, slot);
            let range = tls.map_or(base as u64..base as u64 + 16, |tls| {
                tls as u64..tls as u64 + 4
            });
            let mut stage = StagedExports::prepare_artifact(
                &mut store,
                &artifact,
                &imports,
                memory.clone(),
                table.clone(),
                std::slice::from_ref(&range),
                std::slice::from_ref(&(slot as u64..slot as u64 + 1)),
                tls,
            )
            .unwrap();
            assert_eq!(stage.instance().module(), artifact.module());
            let state = stage
                .instance()
                .exports
                .get_global("__wasix_dylink_global_2")
                .unwrap()
                .clone();
            assert_eq!(
                state.get(&mut store).i32(),
                Some(0),
                "start never runs during instance construction"
            );
            let mut data = [0; 3];
            memory.view(&store).read(base as u64, &mut data).unwrap();
            assert_eq!(data, [0; 3]);
            assert!(matches!(
                table.get(&mut store, slot as u32),
                Some(Value::FuncRef(None))
            ));
            stage.apply(&mut store).unwrap();
            assert_eq!(state.get(&mut store).i32(), Some(7));
            if let Some(tls) = tls {
                let mut bytes = [0; 4];
                memory.view(&store).read(tls as u64, &mut bytes).unwrap();
                assert_eq!(u32::from_le_bytes(bytes), 11);
                memory.view(&store).read(base as u64, &mut data).unwrap();
                assert_eq!(
                    data, [0; 3],
                    "peer TLS replay must not reinitialize shared data"
                );
            } else {
                memory.view(&store).read(base as u64, &mut data).unwrap();
                assert_eq!(&data, b"new");
            }
            published.push((store, stage.publish().unwrap(), base));
            assert_eq!(
                compilations.load(Ordering::Relaxed),
                1,
                "another store compiled the artifact again"
            );
        }
        drop(artifact);
        assert!(
            weak.upgrade().is_none(),
            "staged instances do not pin the loader artifact"
        );
        for (mut store, instance, base) in published {
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "value")
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                base
            );
        }
        assert_eq!(compilations.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn cached_artifact_still_checks_each_store_reservations_identity_and_rollback() {
        let (engine, compilations) = engine();
        let artifact = PreparedArtifact::compile(&engine, initialization()).unwrap();
        let (mut store, memory, table, imports) = fixture(&engine, 128, 3);
        assert!(
            StagedExports::prepare_artifact(
                &mut store,
                &artifact,
                &imports,
                memory.clone(),
                table.clone(),
                std::slice::from_ref(&(129..144)),
                std::slice::from_ref(&(3..4)),
                None
            )
            .is_err()
        );
        let other = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
        assert!(
            StagedExports::prepare_artifact(
                &mut store,
                &artifact,
                &imports,
                other,
                table.clone(),
                std::slice::from_ref(&(128..144)),
                std::slice::from_ref(&(3..4)),
                None
            )
            .is_err()
        );
        let mut stage = StagedExports::prepare_artifact(
            &mut store,
            &artifact,
            &imports,
            memory.clone(),
            table.clone(),
            std::slice::from_ref(&(128..144)),
            std::slice::from_ref(&(3..4)),
            None,
        )
        .unwrap();
        stage.apply(&mut store).unwrap();
        stage.rollback(&mut store).unwrap();
        let mut bytes = [0; 3];
        memory.view(&store).read(128, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 3]);
        assert!(matches!(
            table.get(&mut store, 3),
            Some(Value::FuncRef(None))
        ));
        assert_eq!(compilations.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn private_table_is_rejected_before_compilation() {
        let (engine, compilations) = engine();
        let initialization = Arc::new(
            Initialization::parse(&crate::tests::bytes(
                "(module (table 3 funcref))",
                [0; 4],
                &[],
                &[],
                &[],
            ))
            .unwrap(),
        );
        assert!(
            PreparedArtifact::compile(&engine, initialization)
                .err()
                .unwrap()
                .to_string()
                .contains("private function table")
        );
        assert_eq!(compilations.load(Ordering::Relaxed), 0);
    }
}

#[cfg(all(test, feature = "js", target_arch = "wasm32"))]
mod js_tests {
    use super::{
        PreparedArtifact,
        test_fixture::{fixture, initialization},
    };
    use crate::store::{BoundModule, StagedExports};
    use std::sync::Arc;
    use wasm_bindgen_test::wasm_bindgen_test;
    use wasmer::{Engine, Value};

    #[wasm_bindgen_test]
    fn prepared_js_artifact_reuses_one_module_with_per_store_effects_and_rollback() {
        fn send_sync<T: Send + Sync>() {}
        fn send<T: Send>() {}
        send_sync::<BoundModule>();
        send::<StagedExports>();
        let engine = Engine::default();
        let fallbacks = wasmer::js::shared_object_stats().1;
        let artifact = Arc::new(PreparedArtifact::compile(&engine, initialization()).unwrap());
        let original: js_sys::WebAssembly::Module = artifact.module().clone().into();
        assert_eq!(wasmer::js::shared_object_stats().1, fallbacks);
        let weak = Arc::downgrade(&artifact);
        let mut published = Vec::new();
        for (base, slot, tls) in [(128, 3, None), (256, 5, None), (384, 7, Some(512))] {
            let (mut store, memory, table, imports) = fixture(&engine, base, slot);
            let range = tls.map_or(base as u64..base as u64 + 16, |tls| {
                tls as u64..tls as u64 + 4
            });
            let table_range = slot as u64..slot as u64 + 1;
            let outside = range.start + 1..range.end;
            assert!(
                StagedExports::prepare_artifact(
                    &mut store,
                    &artifact,
                    &imports,
                    memory.clone(),
                    table.clone(),
                    std::slice::from_ref(&outside),
                    std::slice::from_ref(&table_range),
                    tls
                )
                .is_err()
            );
            let mut stage = StagedExports::prepare_artifact(
                &mut store,
                &artifact,
                &imports,
                memory.clone(),
                table.clone(),
                std::slice::from_ref(&range),
                std::slice::from_ref(&table_range),
                tls,
            )
            .unwrap();
            let same: js_sys::WebAssembly::Module = stage.instance().module().clone().into();
            assert!(
                js_sys::Object::is(original.as_ref(), same.as_ref()),
                "another store compiled/replaced the prepared JS Module"
            );
            let state = stage
                .instance()
                .exports
                .get_global("__wasix_dylink_global_2")
                .unwrap()
                .clone();
            assert_eq!(
                state.get(&mut store).i32(),
                Some(0),
                "auto-start must remain removed"
            );
            let mut bytes = [0; 4];
            memory.view(&store).read(range.start, &mut bytes).unwrap();
            assert_eq!(bytes, [0; 4]);
            assert!(matches!(
                table.get(&mut store, slot as u32),
                Some(Value::FuncRef(None))
            ));
            stage.apply(&mut store).unwrap();
            assert_eq!(state.get(&mut store).i32(), Some(7));
            if let Some(tls) = tls {
                memory.view(&store).read(tls as u64, &mut bytes).unwrap();
                assert_eq!(u32::from_le_bytes(bytes), 11);
                let mut shared = [0; 3];
                memory.view(&store).read(base as u64, &mut shared).unwrap();
                assert_eq!(
                    shared, [0; 3],
                    "TLS replay must not reinitialize shared data"
                );
            } else {
                let mut data = [0; 3];
                memory.view(&store).read(base as u64, &mut data).unwrap();
                assert_eq!(&data, b"new");
            }
            stage.rollback(&mut store).unwrap();
            memory.view(&store).read(range.start, &mut bytes).unwrap();
            assert_eq!(bytes, [0; 4]);
            assert!(matches!(
                table.get(&mut store, slot as u32),
                Some(Value::FuncRef(None))
            ));
            drop(stage);
            // Re-preparation creates fresh store-local globals/instances, still using
            // the exact same compiled module, after an abandoned unpublished attempt.
            let mut retry = StagedExports::prepare_artifact(
                &mut store,
                &artifact,
                &imports,
                memory.clone(),
                table.clone(),
                std::slice::from_ref(&range),
                std::slice::from_ref(&table_range),
                tls,
            )
            .unwrap();
            assert_eq!(
                retry
                    .instance()
                    .exports
                    .get_global("__wasix_dylink_global_2")
                    .unwrap()
                    .get(&mut store)
                    .i32(),
                Some(0)
            );
            let same: js_sys::WebAssembly::Module = retry.instance().module().clone().into();
            assert!(js_sys::Object::is(original.as_ref(), same.as_ref()));
            retry.apply(&mut store).unwrap();
            published.push((store, retry.publish().unwrap(), base));
            assert_eq!(
                wasmer::js::shared_object_stats().1,
                fallbacks,
                "same-realm artifact reuse must not invoke the cached-realm fallback"
            );
        }
        drop(artifact);
        assert!(weak.upgrade().is_none());
        for (mut store, instance, base) in published {
            let same: js_sys::WebAssembly::Module = instance.module().clone().into();
            assert!(js_sys::Object::is(original.as_ref(), same.as_ref()));
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "value")
                    .unwrap()
                    .call(&mut store)
                    .unwrap(),
                base
            );
        }
        assert_eq!(wasmer::js::shared_object_stats().1, fallbacks);
    }
}
