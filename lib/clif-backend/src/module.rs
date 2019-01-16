use wasmer_runtime::{
    backend::SigRegistry,
    memory::LinearMemory,
    module::{
        DataInitializer, ExportIndex, ImportName, ModuleInner, TableInitializer,
    },
    types::{
        ElementType, FuncIndex, FuncSig,
        Global, GlobalDesc, GlobalIndex,
        Initializer, Map, TypedIndex, Memory,
        MemoryIndex as WasmerMemoryIndex, SigIndex as WasmerSignatureIndex, Table as WasmerTable,
        TableIndex as WasmerTableIndex, Type as WasmerType, GlobalInit as WasmerGlobalInit,
    },
    vm::{self, Ctx as WasmerVMContext},
};

/// This is a wasmer module.
pub struct Module {

}