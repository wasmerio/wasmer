use std::{
    sync::Arc,
};
use wasmer_runtime_core::{
    module::ModuleInfo,
    types::LocalFuncIndex,
    structures::BoxedMap,
};
use wasmparser::{FunctionBody, CodeSectionReader, BinaryReaderError, Range};
use crate::{
    alloc_pool::{AllocId, AllocPool},
    code::Code,
};

pub struct FunctionBodies {
    binary: Vec<u8>,
    offsets: BoxedMap<LocalFuncIndex, usize>,
}

impl FunctionBodies {
    fn new(code_reader: CodeSectionReader) -> Result<Self, BinaryReaderError> {
        
        let offsets = code_reader.into_iter().map(|body| {
            let body = body?;
            let Range { start, .. } = body.range();
            start
        })
        for body in code_reader {
            let body = body?;
            let Range { start, .. } = body.range();

        }
    }

    pub fn at(&self, index: LocalFuncIndex) -> FunctionBody {
        let offset = self.offsets[index];
        FunctionBody::new(offset, &self.binary)
    }
}

pub struct ModuleContext {
    alloc_pool: Arc<AllocPool>,
    info: ModuleInfo,
    functions: FunctionBodies,
}