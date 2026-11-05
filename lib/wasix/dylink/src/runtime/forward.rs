//! Typed edges and unresolved-call traps, all using ordinary exec instrumentation.
use super::{Result, failure, lock};
use std::{collections::BTreeMap, sync::Mutex};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, Module, RefType, TableType, TypeSection,
    reencode::{Reencode, RoundtripReencoder},
};
use wasmer::{AsStoreMut, AsStoreRef, Imports, Instance, Table};
use wasmer_types::ModuleHash;
use wasmparser::FuncType;

type Key = (ModuleHash, String, String);

// JS export descriptors cannot preserve Wasm declaration order for integer keys.
pub(super) fn name(index: usize) -> String {
    format!("__wasix_dylink_forward_{index}")
}

/// Image-owned code variants, never store-local functions or instances. Match v1's
/// SharedCache engine/profile key; final bytes include signatures, slots and table limits.
#[derive(Default)]
pub(super) struct Cache {
    modules: Mutex<BTreeMap<Key, wasmer::Module>>,
}
impl Cache {
    pub fn clear(&self) {
        lock(&self.modules).clear();
    }
    fn module(&self, store: &impl AsStoreRef, bytes: &[u8]) -> Result<wasmer::Module> {
        let store = store.as_store_ref();
        let engine = store.engine();
        let key = (
            ModuleHash::new(bytes),
            engine.deterministic_id(),
            engine.artifact_format(),
        );
        let mut modules = lock(&self.modules);
        if let Some(module) = modules.get(&key) {
            return Ok(module.clone());
        }
        // Serialize misses for the same image so peers never duplicate compilation.
        // There are no guest calls/awaits here, and no lock spans instantiation.
        let module = wasmer::Module::new(&store, bytes).map_err(failure)?;
        modules.insert(key, module.clone());
        Ok(module)
    }
}

pub(super) fn instantiate(
    cache: &Cache,
    store: &mut impl AsStoreMut,
    table: &Table,
    imports: &Imports,
    targets: &[(FuncType, u32)],
) -> Result<Instance> {
    instantiate_targets(
        cache,
        store,
        table,
        imports,
        &targets
            .iter()
            .map(|(ty, slot)| (ty.clone(), Some(*slot)))
            .collect::<Vec<_>>(),
    )
}

pub(super) fn traps(
    cache: &Cache,
    store: &mut impl AsStoreMut,
    table: &Table,
    imports: &Imports,
    signatures: &[FuncType],
) -> Result<Instance> {
    instantiate_targets(
        cache,
        store,
        table,
        imports,
        &signatures
            .iter()
            .map(|ty| (ty.clone(), None))
            .collect::<Vec<_>>(),
    )
}

fn instantiate_targets(
    cache: &Cache,
    store: &mut impl AsStoreMut,
    table: &Table,
    imports: &Imports,
    targets: &[(FuncType, Option<u32>)],
) -> Result<Instance> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();
    for (index, (signature, slot)) in targets.iter().enumerate() {
        if slot.is_some_and(|slot| slot < 3) {
            return Err(failure("forwarding target is a signal sentinel"));
        }
        let convert = |ty| RoundtripReencoder.val_type(ty).map_err(failure);
        types.ty().function(
            signature
                .params()
                .iter()
                .copied()
                .map(convert)
                .collect::<Result<Vec<_>>>()?,
            signature
                .results()
                .iter()
                .copied()
                .map(convert)
                .collect::<Result<Vec<_>>>()?,
        );
        let index = u32::try_from(index).map_err(failure)?;
        functions.function(index);
        exports.export(&name(index as usize), ExportKind::Func, index);
        let mut function = Function::new([]);
        if let Some(slot) = slot {
            for argument in 0..signature.params().len() {
                function.instruction(&Instruction::LocalGet(
                    u32::try_from(argument).map_err(failure)?,
                ));
            }
            function.instruction(&Instruction::I32Const(*slot as i32));
            function.instruction(&Instruction::CallIndirect {
                type_index: index,
                table_index: 0,
            });
        } else {
            // exec fault_signal maps unreachable to the existing process SIGILL path.
            function.instruction(&Instruction::Unreachable);
        }
        function.instruction(&Instruction::End);
        code.function(&function);
    }
    let ty = table.ty(&store.as_store_ref());
    let mut imported = ImportSection::new();
    imported.import(
        "env",
        "__indirect_function_table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            minimum: u64::from(ty.minimum),
            maximum: ty.maximum.map(u64::from),
            table64: false,
            shared: false,
        }),
    );
    module.section(&types);
    module.section(&imported);
    module.section(&functions);
    module.section(&exports);
    module.section(&code);
    // Forwarders use ordinary exec instrumentation, never a trusted-helper exemption.
    let bytes = wasixv2_exec::instrument(&module.finish()).map_err(failure)?;
    let module = cache.module(&store.as_store_ref(), &bytes)?;
    let mut imports = imports.clone();
    imports.define("env", "__indirect_function_table", table.clone());
    Instance::new(store, &module, &imports).map_err(failure)
}

#[cfg(all(test, feature = "engine-tests"))]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use wasmer::{
        Engine, Function as HostFunction, Store, TableType as HostTableType, Type, Value,
        sys::{CompilerConfig, Cranelift, EngineBuilder, FunctionMiddleware, ModuleMiddleware},
    };
    use wasmer_types::{LocalFunctionIndex, MiddlewareError, ModuleInfo};

    #[derive(Debug)]
    struct Count(Arc<AtomicUsize>);
    #[derive(Debug)]
    struct Pass;
    impl<'a> FunctionMiddleware<'a> for Pass {}
    impl ModuleMiddleware for Count {
        fn generate_function_middleware<'a>(
            &self,
            _: LocalFunctionIndex,
        ) -> Box<dyn FunctionMiddleware<'a> + 'a> {
            Box::new(Pass)
        }
        fn transform_module_info(
            &self,
            _: &mut ModuleInfo,
        ) -> std::result::Result<(), MiddlewareError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    fn engine() -> (Engine, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let mut compiler = Cranelift::default();
        compiler.push_middleware(Arc::new(Count(count.clone())));
        (EngineBuilder::new(compiler).engine().into(), count)
    }
    fn fixture(
        engine: &Engine,
        minimum: u32,
        maximum: u32,
        offset: i32,
    ) -> (Store, Table, Imports) {
        let mut store = Store::new(engine.clone());
        let table = Table::new(
            &mut store,
            HostTableType::new(Type::FuncRef, minimum, Some(maximum)),
            Value::FuncRef(None),
        )
        .unwrap();
        let function = HostFunction::new_typed(&mut store, move |value: i32| value + offset);
        table
            .set(&mut store, 3, Value::FuncRef(Some(function)))
            .unwrap();
        let mut imports = Imports::new();
        imports.define(
            "wasixv2-exec",
            "preempt",
            HostFunction::new_typed(&mut store, || 65536i32),
        );
        imports.define(
            "wasixv2-exec",
            "unreachable",
            HostFunction::new_typed(&mut store, || 0i32),
        );
        imports.define(
            "wasixv2-exec",
            "memory-grow",
            HostFunction::new_typed(&mut store, |_: i32| -1i32),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-wait32",
            HostFunction::new_typed(&mut store, |_: i32, _: i32, _: i64, _: i32| -> i32 {
                panic!("unexpected wait32")
            }),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-wait64",
            HostFunction::new_typed(&mut store, |_: i32, _: i64, _: i64, _: i32| -> i32 {
                panic!("unexpected wait64")
            }),
        );
        imports.define(
            "wasixv2-exec",
            "memory-atomic-notify",
            HostFunction::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 {
                panic!("unexpected notify")
            }),
        );
        (store, table, imports)
    }
    fn signature() -> FuncType {
        FuncType::new([wasmparser::ValType::I32], [wasmparser::ValType::I32])
    }
    #[test]
    fn cached_forwarder_compiles_once_but_uses_each_stores_own_function_table() {
        let (engine, compilations) = engine();
        let cache = Cache::default();
        let mut live = Vec::new();
        for offset in [10, 20, 30] {
            let (mut store, table, imports) = fixture(&engine, 4, 8, offset);
            let instance =
                instantiate(&cache, &mut store, &table, &imports, &[(signature(), 3)]).unwrap();
            let function = instance
                .exports
                .get_typed_function::<i32, i32>(&store, &name(0))
                .unwrap();
            assert_eq!(function.call(&mut store, 2).unwrap(), offset + 2);
            assert_eq!(compilations.load(Ordering::Relaxed), 1);
            live.push((store, instance, offset));
        }
        assert_eq!(lock(&cache.modules).len(), 1);
        cache.clear();
        assert!(lock(&cache.modules).is_empty());
        for (mut store, instance, offset) in live {
            assert_eq!(
                instance
                    .exports
                    .get_typed_function::<i32, i32>(&store, &name(0))
                    .unwrap()
                    .call(&mut store, 5)
                    .unwrap(),
                offset + 5,
                "clearing loader ownership must not invalidate a live instance"
            );
        }
        let (mut store, table, imports) = fixture(&engine, 4, 8, 40);
        instantiate(&cache, &mut store, &table, &imports, &[(signature(), 3)]).unwrap();
        assert_eq!(compilations.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn cache_separates_slots_table_limits_and_unresolved_trap_code() {
        let (engine, compilations) = engine();
        let cache = Cache::default();
        let (mut store, table, imports) = fixture(&engine, 4, 8, 10);
        instantiate(&cache, &mut store, &table, &imports, &[(signature(), 3)]).unwrap();
        instantiate(&cache, &mut store, &table, &imports, &[(signature(), 4)]).unwrap();
        assert_eq!(compilations.load(Ordering::Relaxed), 2);
        for _ in 0..2 {
            let trap = traps(&cache, &mut store, &table, &imports, &[signature()]).unwrap();
            assert!(
                trap.exports
                    .get_typed_function::<i32, i32>(&store, &name(0))
                    .unwrap()
                    .call(&mut store, 1)
                    .is_err()
            );
        }
        assert_eq!(compilations.load(Ordering::Relaxed), 3);
        let (mut store, table, imports) = fixture(&engine, 5, 8, 10);
        instantiate(&cache, &mut store, &table, &imports, &[(signature(), 3)]).unwrap();
        assert_eq!(compilations.load(Ordering::Relaxed), 4);
        let (mut store, table, imports) = fixture(&engine, 5, 16, 10);
        instantiate(&cache, &mut store, &table, &imports, &[(signature(), 3)]).unwrap();
        assert_eq!(compilations.load(Ordering::Relaxed), 5);
        assert_eq!(lock(&cache.modules).len(), 5);
    }
}
