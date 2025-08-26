use crate::state::{ModuleHandle, ModuleHandleWithFlags, SymbolResolutionResult};
use derive_more::Debug;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FullSymbolCacheKey {
    module_handle: ModuleHandle,
    // Corresponds to the first identifier, such as env in env.memory. Both "module"
    // names come from the WASM spec, unfortunately, so we can't change them.
    // We only resolve from a well-known set of modules, namely "env", "GOT.mem" and
    // "GOT.func", so this doesn't need to be an owned string.
    import_module: String,
    import_name: String,
}

/// The name of the module part of an import. We only recognize a few well-known names.
enum ImportModuleName {
    /// Corresponds to `env`
    Env,
    /// Corresponds to `GOT.mem`
    GotMem,
    /// Corresponds to `GOT.func`
    GotFunc,
}

impl TryFrom<&str> for ImportModuleName {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "env" => Ok(ImportModuleName::Env),
            "GOT.mem" => Ok(ImportModuleName::GotMem),
            "GOT.func" => Ok(ImportModuleName::GotFunc),
            _ => Err(()),
        }
    }
}

/// Provides a fast and flexible cache for dynamic symbol resolutions, so that dl_sym does not have to got through the linker
///
/// This is specifically only for _dynamically_ resolved symbols, i.e. those that are looked up via dlopen/dlsym.
/// Symbols that are resolved at module load time are not the focus of this cache.
pub struct SymbolResolutionCache {
    /// The cache stores a mapping from the symbol name to a set of module handles and results.
    ///
    /// This should be quite efficient as it is uncommon that multiple modules export the same symbol name.
    /// The inner Vec should be in the low single digits in most cases.
    cache: BTreeMap<String, Vec<(ModuleHandle, SymbolResolutionResult)>>,
}

impl SymbolResolutionCache {
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
        }
    }

    /// Insert a new entry into the cache. If an entry already exists, it is overwritten.
    ///
    /// The `module_handle` and
    pub fn insert_entry(
        &mut self,
        module_handle: &ModuleHandle,
        name: &str,
        result: SymbolResolutionResult,
    ) {
        self.cache
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push((*module_handle, result));
        // TODO: Maybe check if there is already an entry with the same type in the Vec and replace it instead of pushing a new one.
    }

    pub fn get_fully_specified(
        &self,
        module_handle: &ModuleHandle,
        module: &str,
        name: &str,
    ) -> Option<&SymbolResolutionResult> {
        let import_module = ImportModuleName::try_from(module).ok()?;

        let entries = self.cache.get(name)?;

        entries.iter().find_map(|(handle, entry)| {
            if *handle != *module_handle {
                return None;
            }
            // TODO: Figure out if we need to handle special cases like __stack_pointer, tables and memories here.
            match (&import_module, &entry) {
                (ImportModuleName::Env, SymbolResolutionResult::Function { .. }) => Some(entry),
                (ImportModuleName::GotMem, SymbolResolutionResult::Memory { .. }) => Some(entry),
                (ImportModuleName::GotFunc, SymbolResolutionResult::FunctionPointer { .. }) => {
                    Some(entry)
                }
                _ => None,
            }
        })
    }

    /// This is the replacement for RTLD_DEFAULT. For now the implementation is wrong.
    /// TODO: This only searches through the current module and not through the tree
    pub fn get_by_name(&self, name: &str) -> Option<&SymbolResolutionResult> {
        let result = &self.cache.get(name)?.first()?.1;
        Some(result)
    }

    /// TODO: This only searches through the current module and not through the tree
    pub fn get_by_module_and_name(
        &self,
        name: &str,
        module: &ModuleHandle,
    ) -> Option<&SymbolResolutionResult> {
        self.cache
            .get(name)?
            .iter()
            .find_map(|(module_handle, entry)| {
                if module_handle == module {
                    Some(entry)
                } else {
                    None
                }
            })
    }

    pub fn get_by_name_and_module_handle(
        &self,
        name: &str,
        module_handle: &ModuleHandleWithFlags,
    ) -> Option<&SymbolResolutionResult> {
        match module_handle {
            ModuleHandleWithFlags::Normal(module_handle) => {
                self.get_by_module_and_name(name, module_handle)
            }
            ModuleHandleWithFlags::RtldDefault => self.get_by_name(name),
            &ModuleHandleWithFlags::Invalid => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &SymbolResolutionResult)> {
        let x = self
            .cache
            .iter()
            .flat_map(|(name, v)| v.iter().map(|(_, r)| (name.as_str(), r)));
        // let y = x.next();
        // todo!();
        x
    }

    /// Iterate over all function pointers in the cache, returning the symbol name, the module handle it was resolved from and the function table index.
    /// 
    /// This is useful for restoring the function table when starting a new instance group.
    pub fn all_function_pointers_iter(&self) -> impl Iterator<Item = (&str, &ModuleHandle, &u32)> {
        self.cache.iter().flat_map(|(name, v)| {
            v.iter().filter_map(|(_, entry)| {
                let SymbolResolutionResult::FunctionPointer {
                    function_table_index,
                    resolved_from,
                } = entry
                else {
                    return None;
                };
                Some((name.as_str(), resolved_from, function_table_index))
            })
        })
    }
}
