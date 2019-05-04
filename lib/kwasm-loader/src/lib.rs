pub mod service;

use wasmer_runtime_core::{
    loader::{self, Loader, Instance},
    backend::RunnableModule,
    vm::{InternalCtx, LocalGlobal},
    module::ModuleInfo,
    types::{Value, LocalMemoryIndex, ImportedMemoryIndex},
    structures::TypedIndex,
};
use service::{ServiceContext, RunProfile};

pub struct KernelLoader;

impl Loader for KernelLoader {
    type Instance = KernelInstance;
    type Error = String;

    fn load(&self, rm: &dyn RunnableModule, module: &ModuleInfo, ctx: &InternalCtx) -> Result<Self::Instance, Self::Error> {
        let code = rm.get_code().unwrap();
        let memory = if let Some(_) = module.memories.get(LocalMemoryIndex::new(0)) {
            Some(unsafe {
                ::std::slice::from_raw_parts((**ctx.memories).base, (**ctx.memories).bound)
            }.to_vec())
        } else if let Some(_) = module.imported_memories.get(ImportedMemoryIndex::new(0)) {
            return Err("imported memory is not supported".into());
        } else {
            None
        };
        if module.imported_globals.len() > 0 {
            return Err("imported globals are not supported".into());
        }
        let globals: Vec<u64> = unsafe {
            let globals: &[*mut LocalGlobal] = ::std::slice::from_raw_parts(ctx.globals, module.globals.len());
            globals.iter().map(|x| (**x).data).collect()
        };
        let mut import_names: Vec<String> = vec![];
        for (_, import) in &module.imported_functions {
            let name = format!("{}##{}", module.namespace_table.get(import.namespace_index), module.name_table.get(import.name_index));
            import_names.push(name);
        }
        Ok(KernelInstance {
            context: ServiceContext::connect().map_err(|x| format!("{:?}", x))?,
            code: code.to_vec(),
            memory: memory,
            globals: globals,
            offsets: rm.get_offsets().unwrap(),
            import_names: import_names,
        })
    }
}

pub struct KernelInstance {
    context: ServiceContext,
    code: Vec<u8>,
    memory: Option<Vec<u8>>,
    globals: Vec<u64>,
    offsets: Vec<usize>,
    import_names: Vec<String>,
}

impl Instance for KernelInstance {
    type Error = String;
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u64, String> {
        let args: Vec<u64> = args.iter().map(|x| x.to_u64()).collect();

        let ret = self.context.run_code(RunProfile {
            code: &self.code,
            memory: if let Some(ref x) = self.memory { Some(&*x) } else { None },
            memory_max: 0,
            globals: &self.globals,
            params: &args,
            entry_offset: self.offsets[id] as u32,
            imports: &self.import_names,
        }).map_err(|x| format!("{:?}", x))?;
        Ok(ret as u64)
    }
}