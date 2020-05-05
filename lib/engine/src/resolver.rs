//! Define the `Resolver` trait, allowing custom resolution for external
//! references.

use crate::error::{ImportError, LinkError};
use more_asserts::assert_ge;
use wasm_common::entity::PrimaryMap;
use wasm_common::{ExternType, ImportIndex, MemoryIndex, TableIndex};
use wasmer_runtime::{
    Export, Imports, SignatureRegistry, VMFunctionImport, VMGlobalImport, VMMemoryImport,
    VMTableImport,
};

use wasmer_runtime::{MemoryPlan, TablePlan};
use wasmer_runtime::{MemoryStyle, Module};

/// Import resolver connects imports with available exported values.
pub trait Resolver {
    /// Resolves an import a WebAssembly module to an export it's hooked up to.
    ///
    /// The `index` provided is the index of the import in the wasm module
    /// that's being resolved. For example 1 means that it's the second import
    /// listed in the wasm module.
    ///
    /// The `module` and `field` arguments provided are the module/field names
    /// listed on the import itself.
    fn resolve(&self, index: u32, module: &str, field: &str) -> Option<Export>;
}

/// `Resolver` implementation that always resolves to `None`.
pub struct NullResolver {}

impl Resolver for NullResolver {
    fn resolve(&self, _idx: u32, _module: &str, _field: &str) -> Option<Export> {
        None
    }
}

/// Get an `ExternType` given a import index.
fn get_extern_from_import(module: &Module, import_index: &ImportIndex) -> ExternType {
    match import_index {
        ImportIndex::Function(index) => {
            let func = module.signatures[module.functions[*index]].clone();
            ExternType::Function(func)
        }
        ImportIndex::Table(index) => {
            let table = module.tables[*index].clone();
            ExternType::Table(table)
        }
        ImportIndex::Memory(index) => {
            let memory = module.memories[*index].clone();
            ExternType::Memory(memory)
        }
        ImportIndex::Global(index) => {
            let global = module.globals[*index].clone();
            ExternType::Global(global)
        }
    }
}

/// Get an `ExternType` given an export (and signatures in case is a function).
fn get_extern_from_export(
    _module: &Module,
    signatures: &SignatureRegistry,
    export: &Export,
) -> ExternType {
    match export {
        Export::Function(ref f) => {
            let func = signatures.lookup(f.signature).unwrap().clone();
            ExternType::Function(func)
        }
        Export::Table(ref t) => {
            let table = t.plan().table.clone();
            ExternType::Table(table)
        }
        Export::Memory(ref m) => {
            let memory = m.plan().memory.clone();
            ExternType::Memory(memory)
        }
        Export::Global(ref g) => {
            let global = g.global.clone();
            ExternType::Global(global)
        }
    }
}

/// This function allows to match all imports of a `Module` with concrete definitions provided by
/// a `Resolver`.
///
/// If all imports are satisfied returns an `Imports` instance required for a module instantiation.
pub fn resolve_imports(
    module: &Module,
    signatures: &SignatureRegistry,
    resolver: &dyn Resolver,
    memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
    _table_plans: &PrimaryMap<TableIndex, TablePlan>,
) -> Result<Imports, LinkError> {
    let mut function_imports = PrimaryMap::with_capacity(module.num_imported_funcs);
    let mut table_imports = PrimaryMap::with_capacity(module.num_imported_tables);
    let mut memory_imports = PrimaryMap::with_capacity(module.num_imported_memories);
    let mut global_imports = PrimaryMap::with_capacity(module.num_imported_globals);

    for ((module_name, field, import_idx), import_index) in module.imports.iter() {
        let resolved = resolver.resolve(*import_idx, module_name, field);
        let import_extern = get_extern_from_import(module, import_index);
        let resolved = match resolved {
            None => {
                return Err(LinkError::Import(
                    module_name.to_string(),
                    field.to_string(),
                    ImportError::UnknownImport(import_extern),
                ));
            }
            Some(r) => r,
        };
        let export_extern = get_extern_from_export(module, signatures, &resolved);
        if !export_extern.is_compatible_with(&import_extern) {
            return Err(LinkError::Import(
                module_name.to_string(),
                field.to_string(),
                ImportError::IncompatibleType(import_extern, export_extern),
            ));
        }
        match resolved {
            Export::Function(ref f) => {
                function_imports.push(VMFunctionImport {
                    body: f.address,
                    vmctx: f.vmctx,
                });
            }
            Export::Table(ref t) => {
                table_imports.push(VMTableImport {
                    definition: t.definition,
                    from: t.from,
                });
            }
            Export::Memory(ref m) => {
                match import_index {
                    ImportIndex::Memory(index) => {
                        // Sanity-check: Ensure that the imported memory has at least
                        // guard-page protections the importing module expects it to have.
                        let export_memory_plan = m.plan();
                        let import_memory_plan = &memory_plans[*index];
                        if let (
                            MemoryStyle::Static { bound },
                            MemoryStyle::Static {
                                bound: import_bound,
                            },
                        ) = (export_memory_plan.style.clone(), &import_memory_plan.style)
                        {
                            assert_ge!(bound, *import_bound);
                        }
                        assert_ge!(
                            export_memory_plan.offset_guard_size,
                            import_memory_plan.offset_guard_size
                        );
                    }
                    _ => {
                        // This should never be reached, as we did compatibility
                        // checks before
                        panic!("Memory resolution didn't matched");
                    }
                }

                memory_imports.push(VMMemoryImport {
                    definition: m.definition,
                    from: m.from,
                });
            }

            Export::Global(ref g) => {
                global_imports.push(VMGlobalImport {
                    definition: g.definition,
                });
            }
        }
    }

    Ok(Imports::new(
        function_imports,
        table_imports,
        memory_imports,
        global_imports,
    ))
}
