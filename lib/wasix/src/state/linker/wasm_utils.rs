use tracing::trace;
use wasmer::{
    AsStoreMut, ExportError, ExternType, Global, GlobalType, ImportType, Instance, MemoryType,
    Module, Table, TableType, Type, Value, WasmTypeList,
};

use super::LinkError;

pub(super) fn get_integer_global_type_from_import(
    import: &ImportType,
) -> Result<GlobalType, LinkError> {
    let import_type = import.ty();
    let ExternType::Global(ty) = import_type else {
        return Err(LinkError::BadImport(
            import.module().to_owned(),
            import.name().to_owned(),
            import_type.clone(),
        ));
    };

    if !matches!(ty.ty, Type::I32 | Type::I64) {
        return Err(LinkError::NonIntegerGlobal(
            import.module().to_owned(),
            import.name().to_owned(),
        ));
    }

    Ok(*ty)
}

pub(super) fn define_integer_global_import(
    store: &mut impl AsStoreMut,
    import: &ImportType,
    value: u64,
) -> Result<Global, LinkError> {
    let ExternType::Global(GlobalType { ty, mutability }) = import.ty() else {
        return Err(LinkError::BadImport(
            import.module().to_string(),
            import.name().to_string(),
            import.ty().clone(),
        ));
    };

    let new_global = if mutability.is_mutable() {
        Global::new_mut
    } else {
        Global::new
    };

    let global = match ty {
        Type::I32 => new_global(store, wasmer::Value::I32(value as i32)),
        Type::I64 => new_global(store, wasmer::Value::I64(value as i64)),
        _ => {
            return Err(LinkError::BadImport(
                import.module().to_string(),
                import.name().to_string(),
                import.ty().clone(),
            ));
        }
    };

    Ok(global)
}

pub(super) fn main_module_function_table_type(
    main_module: &Module,
) -> Result<TableType, LinkError> {
    main_module
        .imports()
        .tables()
        .filter_map(|t| {
            if t.ty().ty == Type::FuncRef
                && t.name() == "__indirect_function_table"
                && t.module() == "env"
            {
                Some(*t.ty())
            } else {
                None
            }
        })
        .next()
        .ok_or(LinkError::MissingMainModuleImport(
            "env.__indirect_function_table".to_string(),
        ))
}

pub(super) fn main_module_memory_type(main_module: &Module) -> Result<MemoryType, LinkError> {
    main_module
        .imports()
        .memories()
        .filter_map(|t| {
            if t.name() == "memory" && t.module() == "env" {
                Some(*t.ty())
            } else {
                None
            }
        })
        .next()
        .ok_or(LinkError::MissingMainModuleImport("env.memory".to_string()))
}

pub(super) fn create_indirect_function_table(
    store: &mut impl AsStoreMut,
    table_type: TableType,
    expected_len: u32,
) -> Result<Table, LinkError> {
    trace!(
        minimum_size = ?table_type.minimum,
        "Creating indirect function table"
    );
    let table = Table::new(store, table_type, Value::FuncRef(None))
        .map_err(LinkError::TableAllocationError)?;

    if table.size(store) < expected_len {
        let current_size = table.size(store);
        let delta = expected_len - current_size;
        trace!(?current_size, ?delta, "Growing indirect function table");
        table
            .grow(store, delta, Value::FuncRef(None))
            .map_err(LinkError::TableAllocationError)?;
    }

    trace!(
        size = table.size(store),
        "Indirect function table initial size"
    );

    Ok(table)
}

pub(super) fn create_main_stack_pointer_global(
    store: &mut impl AsStoreMut,
    main_module: &Module,
    initial_value: u64,
) -> Result<Global, LinkError> {
    let import = main_module
        .imports()
        .find(|i| i.module() == "env" && i.name() == "__stack_pointer")
        .ok_or(LinkError::MissingMainModuleImport(
            "__stack_pointer".to_string(),
        ))?;

    define_integer_global_import(store, &import, initial_value)
}

pub(super) fn set_integer_global(
    store: &mut impl AsStoreMut,
    name: &str,
    global: &Global,
    value: u64,
) -> Result<(), LinkError> {
    match global.ty(store).ty {
        Type::I32 => global
            .set(store, Value::I32(value as i32))
            .map_err(|e| LinkError::GlobalUpdateFailed(name.to_owned(), e))?,
        Type::I64 => global
            .set(store, Value::I64(value as i64))
            .map_err(|e| LinkError::GlobalUpdateFailed(name.to_owned(), e))?,
        _ => {
            // This should be caught by resolve_global_import, so just panic here
            unreachable!("Internal error: expected global of type I32 or I64");
        }
    }

    Ok(())
}

pub(super) fn call_initialization_function<Ret: WasmTypeList>(
    instance: &Instance,
    store: &mut impl AsStoreMut,
    name: &str,
) -> Result<Option<Ret>, LinkError> {
    match instance.exports.get_typed_function::<(), Ret>(store, name) {
        Ok(f) => {
            let ret = f
                .call(store)
                .map_err(|e| LinkError::InitFunctionFailed(name.to_string(), e))?;
            Ok(Some(ret))
        }
        Err(ExportError::Missing(_)) => Ok(None),
        Err(ExportError::IncompatibleType) => {
            Err(LinkError::InitFuncWithInvalidSignature(name.to_string()))
        }
    }
}

pub(super) fn get_tls_base_export(
    instance: &Instance,
    store: &mut impl AsStoreMut,
) -> Result<Option<u64>, LinkError> {
    match instance.exports.get_global("__tls_base") {
        Ok(global) => match global.get(store) {
            Value::I32(x) => Ok(Some(x as u64)),
            Value::I64(x) => Ok(Some(x as u64)),
            _ => Err(LinkError::BadTlsBaseExport),
        },
        Err(ExportError::Missing(_)) => Ok(None),
        Err(ExportError::IncompatibleType) => Err(LinkError::BadTlsBaseExport),
    }
}
