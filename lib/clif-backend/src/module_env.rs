use crate::{
    func_env::FuncEnv,
    module::{Converter, Module},
};
use cranelift_codegen::{ir, isa};
use cranelift_wasm::{self, translate_module, FuncTranslator, ModuleEnvironment};
use wasmer_runtime_core::{
    error::{CompileError, CompileResult},
    module::{DataInitializer, ExportIndex, ImportName, TableInitializer},
    structures::{Map, TypedIndex},
    types::{
        ElementType, Global, GlobalDesc, GlobalIndex, Initializer, LocalFuncIndex, LocalOrImport,
        Memory, SigIndex, Table, Value,
    },
};

pub struct ModuleEnv<'module, 'isa> {
    pub module: &'module mut Module,
    isa: &'isa isa::TargetIsa,
    pub signatures: Map<SigIndex, ir::Signature>,
    globals: Map<GlobalIndex, cranelift_wasm::Global>,
    func_bodies: Map<LocalFuncIndex, ir::Function>,
}

impl<'module, 'isa> ModuleEnv<'module, 'isa> {
    pub fn new(module: &'module mut Module, isa: &'isa isa::TargetIsa) -> Self {
        Self {
            module,
            isa,
            signatures: Map::new(),
            globals: Map::new(),
            func_bodies: Map::new(),
        }
    }

    pub fn translate(mut self, wasm: &[u8]) -> CompileResult<Map<LocalFuncIndex, ir::Function>> {
        translate_module(wasm, &mut self)
            .map_err(|e| CompileError::InternalError { msg: e.to_string() })?;
        Ok(self.func_bodies)
    }
}

impl<'module, 'isa, 'data> ModuleEnvironment<'data> for ModuleEnv<'module, 'isa> {
    /// Get the information needed to produce Cranelift IR for the current target.
    fn target_config(&self) -> isa::TargetFrontendConfig {
        self.isa.frontend_config()
    }

    /// Declares a function signature to the environment.
    fn declare_signature(&mut self, sig: &ir::Signature) {
        self.signatures.push(sig.clone());
        self.module.sig_registry.register(Converter(sig).into());
    }

    /// Return the signature with the given index.
    fn get_signature(&self, sig_index: cranelift_wasm::SignatureIndex) -> &ir::Signature {
        &self.signatures[Converter(sig_index).into()]
    }

    /// Declares a function import to the environment.
    fn declare_func_import(
        &mut self,
        sig_index: cranelift_wasm::SignatureIndex,
        namespace: &'data str,
        name: &'data str,
    ) {
        self.module.func_assoc.push(Converter(sig_index).into());

        // Add import names to list of imported functions
        self.module.imported_functions.push(ImportName {
            namespace: namespace.to_string(),
            name: name.to_string(),
        });
    }

    /// Return the number of imported funcs.
    fn get_num_func_imports(&self) -> usize {
        self.module.imported_functions.len()
    }

    /// Declares the type (signature) of a local function in the module.
    fn declare_func_type(&mut self, sig_index: cranelift_wasm::SignatureIndex) {
        self.module.func_assoc.push(Converter(sig_index).into());
    }

    /// Return the signature index for the given function index.
    fn get_func_type(
        &self,
        func_index: cranelift_wasm::FuncIndex,
    ) -> cranelift_wasm::SignatureIndex {
        Converter(self.module.func_assoc[Converter(func_index).into()]).into()
    }

    /// Declares a global to the environment.
    fn declare_global(&mut self, global: cranelift_wasm::Global) {
        use cranelift_wasm::GlobalInit;

        let desc = GlobalDesc {
            mutable: global.mutability,
            ty: Converter(global.ty).into(),
        };

        let init = match global.initializer {
            GlobalInit::I32Const(x) => Initializer::Const(Value::I32(x)),
            GlobalInit::I64Const(x) => Initializer::Const(Value::I64(x)),
            GlobalInit::F32Const(x) => Initializer::Const(Value::F32(f32::from_bits(x))),
            GlobalInit::F64Const(x) => Initializer::Const(Value::F64(f64::from_bits(x))),
            GlobalInit::GetGlobal(global_index) => {
                // assert!(!desc.mutable); // Can be mutable
                let global_index: GlobalIndex = Converter(global_index).into();
                let imported_global_index = global_index
                    .local_or_import(self.module)
                    .import()
                    .expect("invalid global initializer when declaring an imported global");
                Initializer::GetGlobal(imported_global_index)
            }
            _ => panic!("invalid global initializer when declaring a local global"),
        };

        // Add global ir to the list of globals
        self.module.globals.push(Global { desc, init });

        self.globals.push(global);
    }

    /// Declares a global import to the environment.
    fn declare_global_import(
        &mut self,
        global: cranelift_wasm::Global,
        namespace: &'data str,
        name: &'data str,
    ) {
        assert!(match global.initializer {
            cranelift_wasm::GlobalInit::Import => true,
            _ => false,
        });

        let import_name = ImportName {
            namespace: namespace.to_string(),
            name: name.to_string(),
        };

        let desc = GlobalDesc {
            mutable: global.mutability,
            ty: Converter(global.ty).into(),
        };

        // Add global ir to the list of globals
        self.module.imported_globals.push((import_name, desc));

        self.globals.push(global);
    }

    /// Return the global for the given global index.
    fn get_global(&self, global_index: cranelift_wasm::GlobalIndex) -> &cranelift_wasm::Global {
        &self.globals[Converter(global_index).into()]
    }

    /// Declares a table to the environment.
    fn declare_table(&mut self, table: cranelift_wasm::Table) {
        use cranelift_wasm::TableElementType;
        // Add table ir to the list of tables
        self.module.tables.push(Table {
            ty: match table.ty {
                TableElementType::Func => ElementType::Anyfunc,
                _ => unimplemented!(),
            },
            min: table.minimum,
            max: table.maximum,
        });
    }

    /// Declares a table import to the environment.
    fn declare_table_import(
        &mut self,
        table: cranelift_wasm::Table,
        namespace: &'data str,
        name: &'data str,
    ) {
        use cranelift_wasm::TableElementType;

        let import_name = ImportName {
            namespace: namespace.to_string(),
            name: name.to_string(),
        };

        let imported_table = Table {
            ty: match table.ty {
                TableElementType::Func => ElementType::Anyfunc,
                _ => unimplemented!(),
            },
            min: table.minimum,
            max: table.maximum,
        };

        // Add import names to list of imported tables
        self.module
            .imported_tables
            .push((import_name, imported_table));
    }

    /// Fills a declared table with references to functions in the module.
    fn declare_table_elements(
        &mut self,
        table_index: cranelift_wasm::TableIndex,
        base: Option<cranelift_wasm::GlobalIndex>,
        offset: usize,
        elements: Vec<cranelift_wasm::FuncIndex>,
    ) {
        // Convert Cranelift GlobalIndex to wamser GlobalIndex
        // let base = base.map(|index| WasmerGlobalIndex::new(index.index()));
        let base = match base {
            Some(global_index) => {
                let global_index: GlobalIndex = Converter(global_index).into();
                Initializer::GetGlobal(match global_index.local_or_import(self.module) {
                    LocalOrImport::Import(imported_global_index) => imported_global_index,
                    LocalOrImport::Local(_) => {
                        panic!("invalid global initializer when declaring an imported global")
                    }
                })
            }
            None => Initializer::Const((offset as i32).into()),
        };

        // Add table initializer to list of table initializers
        self.module.elem_initializers.push(TableInitializer {
            table_index: Converter(table_index).into(),
            base,
            elements: elements
                .iter()
                .map(|&func_index| Converter(func_index).into())
                .collect(),
        });
    }

    /// Declares a memory to the environment
    fn declare_memory(&mut self, memory: cranelift_wasm::Memory) {
        self.module.memories.push(Memory {
            min: memory.minimum,
            max: memory.maximum,
            shared: memory.shared,
        });
    }

    /// Declares a memory import to the environment.
    fn declare_memory_import(
        &mut self,
        memory: cranelift_wasm::Memory,
        namespace: &'data str,
        name: &'data str,
    ) {
        let import_name = ImportName {
            namespace: namespace.to_string(),
            name: name.to_string(),
        };

        let memory = Memory {
            min: memory.minimum,
            max: memory.maximum,
            shared: memory.shared,
        };

        // Add import names to list of imported memories
        self.module.imported_memories.push((import_name, memory));
    }

    /// Fills a declared memory with bytes at module instantiation.
    fn declare_data_initialization(
        &mut self,
        memory_index: cranelift_wasm::MemoryIndex,
        base: Option<cranelift_wasm::GlobalIndex>,
        offset: usize,
        data: &'data [u8],
    ) {
        // Convert Cranelift GlobalIndex to wamser GlobalIndex
        let base = match base {
            Some(global_index) => {
                let global_index: GlobalIndex = Converter(global_index).into();
                Initializer::GetGlobal(match global_index.local_or_import(self.module) {
                    LocalOrImport::Import(imported_global_index) => imported_global_index,
                    LocalOrImport::Local(_) => {
                        panic!("invalid global initializer when declaring an imported global")
                    }
                })
            }
            None => Initializer::Const((offset as i32).into()),
        };

        // Add data initializer to list of data initializers
        self.module.data_initializers.push(DataInitializer {
            memory_index: Converter(memory_index).into(),
            base,
            data: data.to_vec(),
        });
    }

    /// Declares a function export to the environment.
    fn declare_func_export(&mut self, func_index: cranelift_wasm::FuncIndex, name: &'data str) {
        self.module.exports.insert(
            name.to_string(),
            ExportIndex::Func(Converter(func_index).into()),
        );
    }
    /// Declares a table export to the environment.
    fn declare_table_export(&mut self, table_index: cranelift_wasm::TableIndex, name: &'data str) {
        self.module.exports.insert(
            name.to_string(),
            ExportIndex::Table(Converter(table_index).into()),
        );
    }
    /// Declares a memory export to the environment.
    fn declare_memory_export(
        &mut self,
        memory_index: cranelift_wasm::MemoryIndex,
        name: &'data str,
    ) {
        self.module.exports.insert(
            name.to_string(),
            ExportIndex::Memory(Converter(memory_index).into()),
        );
    }
    /// Declares a global export to the environment.
    fn declare_global_export(
        &mut self,
        global_index: cranelift_wasm::GlobalIndex,
        name: &'data str,
    ) {
        self.module.exports.insert(
            name.to_string(),
            ExportIndex::Global(Converter(global_index).into()),
        );
    }

    /// Declares a start function.
    fn declare_start_func(&mut self, func_index: cranelift_wasm::FuncIndex) {
        self.module.start_func = Some(Converter(func_index).into());
    }

    /// Provides the contents of a function body.
    fn define_function_body(&mut self, body_bytes: &'data [u8]) -> cranelift_wasm::WasmResult<()> {
        let mut func_translator = FuncTranslator::new();

        let func_body = {
            let mut func_env = FuncEnv::new(self);
            let func_index = self.func_bodies.next_index();
            let name = ir::ExternalName::user(0, func_index.index() as u32);

            let sig = func_env.generate_signature(
                self.get_func_type(Converter(func_index.convert_up(self.module)).into()),
            );

            let mut func = ir::Function::with_name_signature(name, sig);

            func_translator.translate(body_bytes, &mut func, &mut func_env)?;

            func
        };

        // Add function body to list of function bodies.
        self.func_bodies.push(func_body);

        Ok(())
    }
}
