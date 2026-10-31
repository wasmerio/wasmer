//! Typed edges and unresolved-call traps, all using ordinary exec instrumentation.
use super::{Result, failure};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, Module, RefType, TableType, TypeSection,
    reencode::{Reencode, RoundtripReencoder},
};
use wasmer::{AsStoreMut, AsStoreRef, Imports, Instance, Table};
use wasmparser::FuncType;

pub(super) fn instantiate(
    store: &mut impl AsStoreMut,
    table: &Table,
    imports: &Imports,
    targets: &[(FuncType, u32)],
) -> Result<Instance> {
    instantiate_targets(
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
    store: &mut impl AsStoreMut,
    table: &Table,
    imports: &Imports,
    signatures: &[FuncType],
) -> Result<Instance> {
    instantiate_targets(
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
        exports.export(&index.to_string(), ExportKind::Func, index);
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
    let module = wasmer::Module::new(&store.as_store_ref(), bytes).map_err(failure)?;
    let mut imports = imports.clone();
    imports.define("env", "__indirect_function_table", table.clone());
    Instance::new(store, &module, &imports).map_err(failure)
}
