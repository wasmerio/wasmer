use tracing::trace;
use wasmer::{AsStoreMut, Exportable, Extern, ExternType, Function, Global, Instance, Value};

use super::{
    DylinkInfo, InstanceGroupState, LinkerState, MAIN_MODULE_HANDLE, ModuleHandle,
    PartiallyResolvedExport, ResolveError,
};

impl InstanceGroupState {
    pub(in crate::state::linker) fn resolve_exported_symbol(
        &self,
        symbol: &str,
    ) -> Option<(ModuleHandle, &Extern)> {
        if let Some(export) = self
            .main_instance()
            .and_then(|instance| instance.exports.get_extern(symbol))
        {
            trace!(symbol, from = ?MAIN_MODULE_HANDLE, ?export, "Resolved exported symbol");
            Some((MAIN_MODULE_HANDLE, export))
        } else {
            for (handle, dl_instance) in &self.side_instances {
                if let Some(export) = dl_instance.instance.exports.get_extern(symbol) {
                    trace!(symbol, from = ?handle, ?export, "Resolved exported symbol");
                    return Some((*handle, export));
                }
            }

            trace!(symbol, "Failed to resolve exported symbol");
            None
        }
    }

    // Resolve an export down to the "memory address" of the symbol. This is different from
    // `resolve_symbol`, which resolves a WASM export but does not care about its type and
    // does no further processing on the export itself.
    pub(in crate::state::linker) fn resolve_export(
        &self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module_handle: Option<ModuleHandle>,
        symbol: &str,
        allow_hidden: bool,
    ) -> Result<(PartiallyResolvedExport, ModuleHandle), ResolveError> {
        trace!(?module_handle, ?symbol, "Resolving export");
        match module_handle {
            Some(module_handle) => {
                let instance = self
                    .try_instance(module_handle)
                    .ok_or(ResolveError::InvalidModuleHandle)?;
                let tls_base = self.tls_base(module_handle);
                let memory_base = linker_state.memory_base(module_handle);
                let dylink_info = linker_state.dylink_info(module_handle);
                Ok((
                    self.resolve_export_from(
                        store,
                        module_handle,
                        symbol,
                        instance,
                        dylink_info,
                        memory_base,
                        tls_base,
                        allow_hidden,
                    )?,
                    module_handle,
                ))
            }

            None => {
                // TODO: this would be the place to support RTLD_NEXT
                if let Some(instance) = self.main_instance() {
                    match self.resolve_export_from(
                        store,
                        MAIN_MODULE_HANDLE,
                        symbol,
                        instance,
                        &linker_state.main_module_dylink_info,
                        linker_state.memory_base(MAIN_MODULE_HANDLE),
                        self.main_instance_tls_base,
                        allow_hidden,
                    ) {
                        Ok(export) => return Ok((export, MAIN_MODULE_HANDLE)),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                // Iterate over linker.side_modules to ensure we're going over the
                // modules in increasing order of module_handle, A.K.A. the order
                // in which modules were loaded. linker.side_modules is a BTreeMap
                // whereas self.side_instances is a HashMap with undetermined
                // iteration order.
                for (handle, module) in &linker_state.side_modules {
                    let instance = &self.side_instances[handle];
                    match self.resolve_export_from(
                        store,
                        *handle,
                        symbol,
                        &instance.instance,
                        &module.dylink_info,
                        linker_state.memory_base(*handle),
                        instance.tls_base,
                        allow_hidden,
                    ) {
                        Ok(export) => return Ok((export, *handle)),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                trace!(
                    ?module_handle,
                    ?symbol,
                    "Failed to locate symbol after searching all instances"
                );
                Err(ResolveError::MissingExport)
            }
        }
    }

    pub(in crate::state::linker) fn resolve_export_from(
        &self,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
        symbol: &str,
        instance: &Instance,
        dylink_info: &DylinkInfo,
        memory_base: u64,
        tls_base: Option<u64>,
        allow_hidden: bool,
    ) -> Result<PartiallyResolvedExport, ResolveError> {
        trace!(from = ?module_handle, symbol, "Resolving export from instance");
        let export = instance.exports.get_extern(symbol).ok_or_else(|| {
            trace!(from = ?module_handle, symbol, "Not found");
            ResolveError::MissingExport
        })?;

        if !allow_hidden
            && dylink_info
                .export_metadata
                .get(symbol)
                .map(|flags| flags.contains(wasmparser::SymbolFlags::VISIBILITY_HIDDEN))
                .unwrap_or(false)
        {
            return Err(ResolveError::MissingExport);
        }

        match export.ty(store) {
            ExternType::Function(_) => {
                trace!(from = ?module_handle, symbol, "Found function");
                Ok(PartiallyResolvedExport::Function(
                    Function::get_self_from_extern(export).unwrap().clone(),
                ))
            }
            ty @ ExternType::Global(_) => {
                let global = Global::get_self_from_extern(export).unwrap();
                let value = match global.get(store) {
                    Value::I32(value) => value as u64,
                    Value::I64(value) => value as u64,
                    _ => return Err(ResolveError::InvalidExportType(ty.clone())),
                };

                let is_tls = dylink_info
                    .export_metadata
                    .get(symbol)
                    .map(|flags| flags.contains(wasmparser::SymbolFlags::TLS))
                    .unwrap_or(false);

                if is_tls {
                    let Some(tls_base) = tls_base else {
                        return Err(ResolveError::NoTlsBaseGlobalExport);
                    };
                    let final_value = value + tls_base;
                    trace!(
                        from = ?module_handle,
                        symbol,
                        value,
                        offset = value,
                        final_value,
                        "Found TLS global"
                    );
                    Ok(PartiallyResolvedExport::Tls {
                        offset: value,
                        final_addr: final_value,
                    })
                } else {
                    let final_value = value + memory_base;
                    trace!(from = ?module_handle, symbol, value, final_value, "Found global");
                    Ok(PartiallyResolvedExport::Global(final_value))
                }
            }
            ty => Err(ResolveError::InvalidExportType(ty.clone())),
        }
    }
}
