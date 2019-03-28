use crate::{
    alloc_pool::{AllocId, AllocPool},
    code::Code,
    compile_pool::{Job, Mode, Priority},
    pipeline::InfoCollection,
};
use rayon::iter::ParallelIterator;
use std::sync::Arc;
use wasmer_runtime_core::{
    module::ModuleInfo,
    structures::{BoxedMap, Map, TypedIndex},
    types::LocalFuncIndex,
};
use wasmparser::{BinaryReaderError, CodeSectionReader, FunctionBody, Range};

#[derive(Debug, Clone)]
pub struct SharedFunctionBody {
    bytes: Arc<[u8]>,
}

impl SharedFunctionBody {
    pub fn body(&self) -> FunctionBody {
        FunctionBody::new(0, &*self.bytes)
    }
}

/// I'm not a big fan of allocating and copying over the wasm again
/// here, but I'm not sure what's a better solution. Maybe better to
/// integrate this more with the streaming compiler?
/// TODO: Improve performance here.
pub struct FunctionBodies {
    functions: Box<[SharedFunctionBody]>,
}

impl FunctionBodies {
    pub fn at(&self, index: LocalFuncIndex) -> SharedFunctionBody {
        self.functions[index.index()].clone()
    }
}

pub struct ModuleContext {
    alloc_pool: Arc<AllocPool>,
    info: Arc<ModuleInfo>,
    functions: FunctionBodies,
}

impl ModuleContext {
    pub fn new(wasm: &[u8]) -> Result<Self, String> {
        let info_collection = InfoCollection::new(wasm).map_err(|e| format!("{:?}", e))?;
        let (info, initial_compile) = info_collection.run().map_err(|e| format!("{:?}", e))?;

        let info = Arc::new(info);
        let alloc_pool = Arc::new(AllocPool::new());

        let functions_and_code = initial_compile
            .run()
            .map(|tuple| {
                let (func_index, bytes) = tuple?;
                let shared_body = SharedFunctionBody { bytes };

                let code_future = Job::create(
                    Arc::clone(&info),
                    Arc::clone(&alloc_pool),
                    shared_body.clone(),
                    func_index,
                    Priority::Warm,
                    Mode::Baseline,
                );

                Ok((shared_body, code_future))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e: BinaryReaderError| format!("{:?}", e))?;
        
        let (functions, )

        let functions = FunctionBodies {
            functions: 
        };

        Ok(Self {
            alloc_pool,
            info,
            functions,
        })
    }

    pub fn alloc_pool(&self) -> &AllocPool {
        &self.alloc_pool
    }

    pub fn info(&self) -> &ModuleInfo {
        &self.info
    }
}
