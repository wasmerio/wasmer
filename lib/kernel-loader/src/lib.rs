pub mod service;

use service::{ImportInfo, LoadProfile, RunProfile, ServiceContext, TableEntryRequest};
use wasmer_runtime_core::{
    backend::RunnableModule,
    loader::{Instance, Loader},
    module::ModuleInfo,
    structures::TypedIndex,
    types::{
        FuncIndex, ImportedMemoryIndex, ImportedTableIndex, LocalMemoryIndex, LocalTableIndex,
        Value,
    },
    vm::{Anyfunc, Ctx, LocalGlobal, SigId},
};

pub struct KernelLoader;

impl Loader for KernelLoader {
    type Instance = KernelInstance;
    type Error = String;

    fn load(
        &self,
        rm: &dyn RunnableModule,
        module: &ModuleInfo,
        full_ctx: &Ctx,
    ) -> Result<Self::Instance, Self::Error> {
        let ctx = &full_ctx.internal;
        let code = rm.get_code().unwrap();
        let memory = if let Some(_) = module.memories.get(LocalMemoryIndex::new(0)) {
            Some(unsafe {
                ::std::slice::from_raw_parts((**ctx.memories).base, (**ctx.memories).bound)
            })
        } else if let Some(_) = module.imported_memories.get(ImportedMemoryIndex::new(0)) {
            return Err("imported memory is not supported".into());
        } else {
            None
        };
        let table: Option<Vec<TableEntryRequest>> =
            if let Some(_) = module.tables.get(LocalTableIndex::new(0)) {
                Some(unsafe {
                    let table = &**ctx.tables;
                    let elements: &[Anyfunc];
                    #[allow(clippy::cast_ptr_alignment)]
                    {
                        elements =
                            ::std::slice::from_raw_parts(table.base as *const Anyfunc, table.count);
                    }

                    let base_addr = code.as_ptr() as usize;
                    let end_addr = base_addr + code.len();
                    elements
                        .iter()
                        .map(|x| {
                            let func_addr = x.func as usize;
                            TableEntryRequest {
                                offset: if x.func.is_null()
                                    || func_addr < base_addr
                                    || func_addr >= end_addr
                                {
                                    ::std::usize::MAX
                                } else {
                                    x.func as usize - base_addr
                                },
                                sig_id: x.sig_id.0,
                            }
                        })
                        .collect()
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
            let globals: &[*mut LocalGlobal] =
                ::std::slice::from_raw_parts(ctx.globals, module.globals.len());
            globals.iter().map(|x| (**x).data).collect()
        };
        let mut import_names: Vec<ImportInfo> = vec![];
        for (idx, import) in &module.imported_functions {
            let name = format!(
                "{}##{}",
                module.namespace_table.get(import.namespace_index),
                module.name_table.get(import.name_index)
            );
            let sig = module
                .signatures
                .get(*module.func_assoc.get(FuncIndex::new(idx.index())).unwrap())
                .unwrap();
            import_names.push(ImportInfo {
                name: name,
                param_count: sig.params().len(),
            });
        }
        let dynamic_sigindices: &[u32] = unsafe {
            ::std::mem::transmute::<&[SigId], &[u32]>(::std::slice::from_raw_parts(
                ctx.dynamic_sigindices,
                full_ctx.dynamic_sigindice_count(),
            ))
        };
        let param_counts: Vec<usize> = (0..module.func_assoc.len())
            .map(|x| {
                module
                    .signatures
                    .get(*module.func_assoc.get(FuncIndex::new(x)).unwrap())
                    .unwrap()
                    .params()
                    .len()
            })
            .collect();
        let profile = LoadProfile {
            code: code,
            memory: memory,
            memory_max: 0,
            globals: &globals,
            imports: &import_names,
            dynamic_sigindices: dynamic_sigindices,
            table: table.as_ref().map(|x| x.as_slice()),
        };
        let sc = ServiceContext::new(profile).map_err(|x| format!("{:?}", x))?;
        Ok(KernelInstance {
            context: sc,
            offsets: rm.get_offsets().unwrap(),
            param_counts: param_counts,
        })
    }
}

pub struct KernelInstance {
    context: ServiceContext,
    offsets: Vec<usize>,
    param_counts: Vec<usize>, // FIXME: Full signature check
}

impl Instance for KernelInstance {
    type Error = String;
    fn call(&mut self, id: usize, args: &[Value]) -> Result<u64, String> {
        if args.len() != self.param_counts[id] {
            return Err("param count mismatch".into());
        }
        let args: Vec<u64> = args.iter().map(|x| x.to_u64()).collect();

        let ret = self
            .context
            .run_code(RunProfile {
                entry_offset: self.offsets[id] as u32,
                params: &args,
            })
            .map_err(|x| format!("{:?}", x))?;
        Ok(ret)
    }

    fn read_memory(&mut self, offset: u32, len: u32) -> Result<Vec<u8>, String> {
        self.context
            .read_memory(offset, len)
            .map_err(|x| format!("{:?}", x))
    }

    fn write_memory(&mut self, offset: u32, len: u32, buf: &[u8]) -> Result<(), String> {
        self.context
            .write_memory(offset, len, buf)
            .map_err(|x| format!("{:?}", x))
    }
}
