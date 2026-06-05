use std::{collections::HashMap, path::PathBuf};

use derive_more::Debug;
use wasmer::{Function, FunctionType, Global, Module};

use super::{DylinkInfo, ModuleHandle};

#[derive(Debug)]
pub(super) enum PartiallyResolvedExport {
    Function(Function),
    Global(u64),
    Tls {
        // The offset relative to the TLS area of the instance. Kept so we
        // can re-resolve for other instance groups.
        offset: u64,
        // The final address of the symbol for the current instance group.
        final_addr: u64,
    },
}

#[derive(Debug)]
pub(super) enum UnresolvedGlobal {
    // A GOT.mem entry, should be resolved to an exported global from another module.
    Mem(NeededSymbolResolutionKey, Global),
    // A GOT.func entry, should be resolved to the address of an exported function
    // from another module (e.g. an index into __indirect_function_table).
    Func(NeededSymbolResolutionKey, Global),
}

impl UnresolvedGlobal {
    pub(super) fn key(&self) -> &NeededSymbolResolutionKey {
        match self {
            Self::Func(key, _) => key,
            Self::Mem(key, _) => key,
        }
    }

    pub(super) fn global(&self) -> &Global {
        match self {
            Self::Func(_, global) => global,
            Self::Mem(_, global) => global,
        }
    }

    pub(super) fn import_module(&self) -> &str {
        match self {
            Self::Func(..) => "GOT.func",
            Self::Mem(..) => "GOT.mem",
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingFunctionResolutionFromLinkerState {
    pub(super) resolved_from: ModuleHandle,
    pub(super) name: String,
    pub(super) function_table_index: u32,
}

#[derive(Debug)]
pub(super) struct PendingTlsPointer {
    pub(super) global: Global,
    pub(super) resolved_from: ModuleHandle,
    pub(super) offset: u64,
}

// Used only when processing a module load operation from another instance group.
// Note: Non-TLS globals are constant across instance groups, and thus we store
// their value, feeding it into new instance groups directly. In the case of TLS
// symbols, they need to get new values based on each specific instance's __tls_base,
// so they need to be tracked.
// __wasm_apply_data_relocs operates on memory addresses, and so it needs to run
// only once. __wasm_apply_tls_relocs does need to run once per instance group, but
// it's run as part of __wasm_init_tls, which itself is called by __wasix_init_tls.
// In either case, there's no need to call any relocation functions when spawning
// further instances of a module that was already loaded and instantiated once.
#[derive(Debug, Default)]
pub(super) struct PendingResolutionsFromLinker {
    pub(super) functions: Vec<PendingFunctionResolutionFromLinkerState>,
    pub(super) tls: Vec<PendingTlsPointer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct NeededSymbolResolutionKey {
    pub(super) module_handle: ModuleHandle,
    // Corresponds to the first identifier, such as env in env.memory. Both "module"
    // names come from the WASM spec, unfortunately, so we can't change them.
    // We only resolve from a well-known set of modules, namely "env", "GOT.mem" and
    // "GOT.func", so this doesn't need to be an owned string.
    pub(super) import_module: String,
    pub(super) import_name: String,
}

#[derive(Debug)]
pub(super) enum InProgressSymbolResolution {
    Function(ModuleHandle),
    StubFunction(FunctionType),
    // May or may not be a TLS symbol.
    MemGlobal(ModuleHandle),
    FuncGlobal(ModuleHandle),
    UnresolvedMemGlobal,
    UnresolvedFuncGlobal,
}

#[derive(Debug)]
pub(super) struct InProgressModuleLoad {
    pub(super) handle: ModuleHandle,
    pub(super) module: Module,
    pub(super) dylink_info: DylinkInfo,
}

#[derive(Default, Debug)]
pub(super) struct InProgressLinkState {
    // All modules loaded in by this link operation, in the order they were loaded in.
    pub(super) new_modules: Vec<InProgressModuleLoad>,

    // Modules that are currently being loaded in from the FS due to needed sections.
    pub(super) pending_module_paths: Vec<PathBuf>,

    // Collection of intermediate symbol resolution results. This includes functions
    // that have been found but not appended to the function tables yet, as well as
    // unresolved globals.
    pub(super) symbols: HashMap<NeededSymbolResolutionKey, InProgressSymbolResolution>,

    pub(super) unresolved_globals: Vec<UnresolvedGlobal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum SymbolResolutionKey {
    Needed(NeededSymbolResolutionKey),
    Requested {
        // Note: since we don't support module unloading, resolving the same symbol
        // from *every* module will find the same symbol every time, so we can cache
        // the None case as well.
        // TODO: once we implement RTLD_NEXT, that flag should also be taken into
        // account here.
        resolve_from: Option<ModuleHandle>,
        name: String,
    },
}

#[derive(Debug)]
pub(super) enum SymbolResolutionResult {
    // The symbol was resolved to a global address. We don't resolve again because
    // the value of globals and the memory_base for each module and all of its instances
    // is fixed.
    // The case of unresolved globals is not mentioned in this enum, since it can't exist
    // once a link operation is finalized.
    Memory(u64),
    // The symbol was resolved to a global address, but the global is a TLS variable.
    // Each instance of each module has a different TLS area, and TLS symbols must be
    // resolved again every time.
    Tls {
        resolved_from: ModuleHandle,
        offset: u64,
    },
    // The symbol was resolved to a function export with the same name from this module.
    // it is expected that the symbol resolves to an export of the correct type.
    Function {
        ty: FunctionType,
        resolved_from: ModuleHandle,
    },
    // Same deal as above, but a pointer was generated and placed in the function table.
    FunctionPointer {
        resolved_from: ModuleHandle,
        function_table_index: u32,
    },
    // The symbol failed to resolve, but it's a function so we can create a stub. The
    // first call to any stub associated with this symbol must update the resolution
    // record to point to the module the function was resolved from.
    StubFunction(FunctionType),
}

pub(super) struct DlModule {
    pub(super) module: Module,
    pub(super) dylink_info: DylinkInfo,
    pub(super) memory_base: u64,
    pub(super) table_base: u64,
}
