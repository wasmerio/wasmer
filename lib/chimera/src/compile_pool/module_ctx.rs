use crate::{
    alloc_pool::{AllocId, AllocPool},
    code::Code,
    pipeline::InfoCollection,
};
use std::sync::Arc;
use wasmer_runtime_core::{
    module::ModuleInfo,
    structures::{BoxedMap, Map},
    types::LocalFuncIndex,
};
use wasmparser::{BinaryReaderError, CodeSectionReader, FunctionBody, Range};

/// I'm not a big fan of allocating and copying over the wasm again
/// here, but I'm not sure what's a better solution. Maybe better to
/// integrate this more with the streaming compiler?
/// TODO: Improve performance here.
pub struct FunctionBodies {
    functions: BoxedMap<LocalFuncIndex, Box<[u8]>>,
}

impl FunctionBodies {
    fn new(iter: impl Iterator<Item = Box<[u8]>>) -> Self {
        Self {
            functions: iter.collect::<Map<_, _>>().into_boxed_map(),
        }
    }

    pub fn at(&self, index: LocalFuncIndex) -> FunctionBody {
        FunctionBody::new(0, &*self.functions[index])
    }
}

pub struct ModuleContext {
    alloc_pool: Arc<AllocPool>,
    info: ModuleInfo,
    functions: FunctionBodies,
}

impl ModuleContext {
    // pub fn new(wasm: &[u8]) -> Result<Self, String> {
    //     let info_collection = InfoCollection::new(wasm)?;
    //     let (info, initial_compile) = info_collection.run()?;

    // }
}
