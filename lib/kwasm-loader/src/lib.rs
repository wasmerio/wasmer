pub mod service;

use wasmer_runtime_core::{
    loader::{self, Loader, Instance},
    backend::RunnableModule,
    vm::{Ctx, LocalGlobal, SigId, Anyfunc},
    module::ModuleInfo,
    types::{Value, LocalMemoryIndex, LocalTableIndex, ImportedMemoryIndex, ImportedTableIndex},
    structures::TypedIndex,
};
use service::{ServiceContext, RunProfile, TableEntryRequest};

pub struct KernelLoader;

impl Loader for KernelLoader {
    type Instance = KernelInstance;
    type Error = String;

    fn load(&self, rm: &dyn RunnableModule, module: &ModuleInfo, full_ctx: &Ctx) -> Result<Self::Instance, Self::Error> {
        let ctx = &full_ctx.internal;
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
        let table: Option<Vec<TableEntryRequest>> = if let Some(_) = module.tables.get(LocalTableIndex::new(0)) {
            Some(unsafe {
                let table = &**ctx.tables;
                let elements: &[Anyfunc] = ::std::slice::from_raw_parts(table.base as *const Anyfunc, table.count);
                let base_addr = code.as_ptr() as usize;
                let end_addr = base_addr + code.len();
                elements.iter().map(|x| {
                    let func_addr = x.func as usize;
                    TableEntryRequest {
                        offset: if x.func.is_null() || func_addr < base_addr || func_addr >= end_addr {
                            ::std::usize::MAX
                        } else {
                            x.func as usize - base_addr
                        },
                        sig_id: x.sig_id.0,
                    }
                }).collect()
            })
        } else if let Some(_) = module.imported_tables.get(ImportedTableIndex::new(0)) {
            return Err("imported table is not supported".into());
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
            table: table,
            globals: globals,
            offsets: rm.get_offsets().unwrap(),
            import_names: import_names,
            dynamic_sigindices: unsafe {
                ::std::slice::from_raw_parts(ctx.dynamic_sigindices, full_ctx.dynamic_sigindice_count())
            }.iter().map(|x| x.0).collect(),
        })
    }
}

pub struct KernelInstance {
    context: ServiceContext,
    code: Vec<u8>,
    memory: Option<Vec<u8>>,
    table: Option<Vec<TableEntryRequest>>,
    globals: Vec<u64>,
    offsets: Vec<usize>,
    import_names: Vec<String>,
    dynamic_sigindices: Vec<u32>,
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
            dynamic_sigindices: &self.dynamic_sigindices,
            table: self.table.as_ref().map(|x| x.as_slice()),
        }).map_err(|x| format!("{:?}", x))?;
        Ok(ret as u64)
    }
}