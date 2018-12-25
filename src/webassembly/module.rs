//! Implementations of `ModuleEnvironment` (as `Module`) and `FuncEnvironment`
//! This module helps to translate wasm code into Cranelift Codegen IR
//! (that will be later compiled in the WebAssembly instance)
use std::collections::HashMap;
use std::string::String;
use std::vec::Vec;

use cranelift_codegen::cursor::FuncCursor;
use cranelift_codegen::ir::immediates::{Imm64, Offset32, Uimm64};
use cranelift_codegen::ir::types::*;
use cranelift_codegen::ir::{
    self, AbiParam, ArgumentPurpose, ExtFuncData, ExternalName, FuncRef, InstBuilder, Signature,
    TrapCode,
};
use cranelift_codegen::isa::{CallConv, TargetFrontendConfig};
use cranelift_entity::{EntityRef, PrimaryMap};

use cranelift_wasm::{
    translate_module, DefinedFuncIndex, FuncEnvironment as FuncEnvironmentTrait, FuncIndex,
    FuncTranslator, Global, GlobalIndex, GlobalVariable, Memory, MemoryIndex, ModuleEnvironment,
    ReturnMode, SignatureIndex, Table, TableIndex, WasmResult,
};

use super::errors::ErrorKind;
use super::instance::Instance;
use super::memory::LinearMemory;

/// Get the integer type used for representing pointers on this platform.
fn native_pointer_type() -> ir::Type {
    if cfg!(target_pointer_width = "64") {
        ir::types::I64
    } else {
        ir::types::I32
    }
}

/// Number of bytes in a native pointer.
pub fn native_pointer_size() -> i32 {
    if cfg!(target_pointer_width = "64") {
        8
    } else {
        4
    }
}

/// Convert a TlsData offset into a `Offset32` for a global decl.
// fn offset32(offset: usize) -> ir::immediates::Offset32 {
//     assert!(offset <= i32::max_value() as usize);
//     (offset as i32).into()
// }

/// Convert a usize offset into a `Imm64` for an iadd_imm.
fn imm64(offset: usize) -> ir::immediates::Imm64 {
    (offset as i64).into()
}

/// Compute a `ir::ExternalName` for a given wasm function index.
fn get_func_name(func_index: FuncIndex) -> ir::ExternalName {
    ir::ExternalName::user(0, func_index.index() as u32)
}

/// A collection of names under which a given entity is imported/exported.
#[derive(Debug)]
pub struct ImportableExportable<T> {
    /// An entity.
    pub entity: T,

    /// Names under which the entity is exported.
    pub export_names: Vec<String>,

    /// Names under which the entity is imported.
    pub import_name: Option<(String, String)>,
}

impl<T> ImportableExportable<T> {
    pub fn new(entity: T, import_name: Option<(String, String)>) -> Self {
        Self {
            entity,
            export_names: Vec::new(),
            import_name,
        }
    }
}

/// An entity to export.
#[derive(Clone, Debug)]
pub enum Export {
    /// Function export.
    Function(FuncIndex),
    /// Table export.
    Table(TableIndex),
    /// Memory export.
    Memory(MemoryIndex),
    /// Global export.
    Global(GlobalIndex),
}

/// TODO: Need to get rid of unused field
/// The main state belonging to a `Module`. This is split out from
/// `Module` to allow it to be borrowed separately from the
/// `FuncTranslator` field.
pub struct ModuleInfo {
    /// Target description relevant to frontends producing Cranelift IR.
    config: TargetFrontendConfig,

    pub main_memory_base: Option<ir::GlobalValue>,

    /// The Cranelift global holding the base address of the memories vector.
    pub memory_base: Option<ir::GlobalValue>,

    /// Signatures as provided by `declare_signature`.
    pub signatures: Vec<ir::Signature>,

    /// Functions, imported and local.
    pub functions: PrimaryMap<FuncIndex, ImportableExportable<SignatureIndex>>,

    /// Function bodies.
    pub function_bodies: PrimaryMap<DefinedFuncIndex, ir::Function>,

    /// Module and field names of imported functions as provided by `declare_func_import`.
    pub imported_funcs: Vec<(String, String)>,

    /// Tables as provided by `declare_table`.
    pub tables: Vec<ImportableExportable<Table>>,

    /// WebAssembly table initializers.
    pub table_elements: Vec<TableElements>,

    /// The base of tables.
    pub tables_base: Option<ir::GlobalValue>,

    /// Memories as provided by `declare_memory`.
    pub memories: Vec<ImportableExportable<Memory>>,

    /// The Cranelift global holding the base address of the globals vector.
    pub globals_base: Option<ir::GlobalValue>,

    /// Globals as provided by `declare_global`.
    pub globals: Vec<ImportableExportable<Global>>,

    /// The start function.
    pub start_func: Option<FuncIndex>,

    /// The data initializers
    pub data_initializers: Vec<DataInitializer>,

    /// Exported entities
    /// We use this in order to have a O(1) allocation of the exports
    /// rather than iterating through the ImportableExportable elements.
    pub exports: HashMap<String, Export>,

    /// The external function declaration for implementing wasm's `current_memory`.
    pub current_memory_extfunc: Option<FuncRef>,

    /// The external function declaration for implementing wasm's `grow_memory`.
    pub grow_memory_extfunc: Option<FuncRef>,
}

impl ModuleInfo {
    /// Allocates the data structures with the given flags.
    pub fn new(config: TargetFrontendConfig) -> Self {
        Self {
            config,
            signatures: Vec::new(),
            functions: PrimaryMap::new(),
            function_bodies: PrimaryMap::new(),
            imported_funcs: Vec::new(),
            tables: Vec::new(),
            memories: Vec::new(),
            globals: Vec::new(),
            globals_base: None,
            table_elements: Vec::new(),
            tables_base: None,
            start_func: None,
            data_initializers: Vec::new(),
            main_memory_base: None,
            memory_base: None,
            exports: HashMap::new(),
            current_memory_extfunc: None,
            grow_memory_extfunc: None,
        }
    }
}

/// A data initializer for linear memory.
#[derive(Debug)]
pub struct DataInitializer {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,
    /// Optionally a globalvalue base to initialize at.
    pub base: Option<GlobalIndex>,
    /// A constant offset to initialize at.
    pub offset: usize,
    /// The initialization data.
    pub data: Vec<u8>,
}

/// Possible values for a WebAssembly table element.
#[derive(Clone, Debug)]
pub enum TableElement {
    /// A element that, if called, produces a trap.
    Trap(),
    /// A function.
    Function(FuncIndex),
}

/// A WebAssembly table initializer.
#[derive(Clone, Debug)]
pub struct TableElements {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: usize,
    /// The values to write into the table elements.
    pub elements: Vec<FuncIndex>,
}

/// This `ModuleEnvironment` implementation is a "naïve" one, doing essentially nothing and
/// emitting placeholders when forced to. Don't try to execute code translated for this
/// environment, essentially here for translation debug purposes.
pub struct Module {
    /// Module information.
    pub info: ModuleInfo,

    /// Function translation.
    trans: FuncTranslator,

    /// Vector of wasm bytecode size for each function.
    pub func_bytecode_sizes: Vec<usize>,
    // How to return from functions.
    // return_mode: ReturnMode,
}

impl Module {
    /// Instantiate a Module given WASM bytecode
    pub fn from_bytes(
        buffer_source: Vec<u8>,
        config: TargetFrontendConfig,
    ) -> Result<Self, ErrorKind> {
        // let return_mode = ReturnMode::NormalReturns;
        let mut module = Self {
            info: ModuleInfo::new(config),
            trans: FuncTranslator::new(),
            func_bytecode_sizes: Vec::new(),
            // return_mode,
        };

        // We iterate through the source bytes, generating the compiled module
        translate_module(&buffer_source, &mut module)
            .map_err(|e| ErrorKind::CompileError(e.to_string()))?;

        Ok(module)
    }

    /// Return a `FuncEnvironment` for translating functions within this
    /// `Module`.
    pub fn func_env(&self) -> FuncEnvironment {
        FuncEnvironment::new(&self.info) //, self.return_mode)
    }

    // fn native_pointer(&self) -> ir::Type {
    //     self.func_env().pointer_type()
    // }

    /// Convert a `DefinedFuncIndex` into a `FuncIndex`.
    pub fn func_index(&self, defined_func: DefinedFuncIndex) -> FuncIndex {
        FuncIndex::new(self.info.imported_funcs.len() + defined_func.index())
    }

    /// Convert a `FuncIndex` into a `DefinedFuncIndex`. Returns None if the
    /// index is an imported function.
    pub fn defined_func_index(&self, func: FuncIndex) -> Option<DefinedFuncIndex> {
        if func.index() < self.info.imported_funcs.len() {
            None
        } else {
            Some(DefinedFuncIndex::new(
                func.index() - self.info.imported_funcs.len(),
            ))
        }
    }

    pub fn verify(&self) {
        unimplemented!();
        // let isa = isa::lookup(self.info.triple.clone())
        //     .unwrap()
        //     .finish(self.info.flags.clone());

        // for func in self.info.function_bodies.values() {
        //     verifier::verify_function(func, &*isa)
        //         .map_err(|errors| panic!(pretty_verifier_error(func, Some(&*isa), None, errors)))
        //         .unwrap();
        // }
    }
}

/// The `FuncEnvironment` implementation for use by the `Module`.
pub struct FuncEnvironment<'environment> {
    pub mod_info: &'environment ModuleInfo,
    // return_mode: ReturnMode,
}

impl<'environment> FuncEnvironment<'environment> {
    pub fn new(mod_info: &'environment ModuleInfo) -> Self {
        // , return_mode: ReturnMode
        Self {
            mod_info,
            // return_mode,
        }
    }

    // fn get_real_call_args(func: &Function, call_args: &[ir::Value]) -> Vec<ir::Value> {
    //     let mut real_call_args = Vec::with_capacity(call_args.len() + 1);
    //     real_call_args.extend_from_slice(call_args);
    //     real_call_args.push(func.special_param(ArgumentPurpose::VMContext).unwrap());
    //     real_call_args
    // }

    // Create a signature for `sigidx` amended with a `vmctx` argument after the standard wasm
    // arguments.
    fn vmctx_sig(&self, sigidx: SignatureIndex) -> ir::Signature {
        let mut sig = self.mod_info.signatures[sigidx.index()].clone();
        sig.params.push(ir::AbiParam::special(
            self.pointer_type(),
            ir::ArgumentPurpose::VMContext,
        ));
        sig
    }
}

impl<'environment> FuncEnvironmentTrait for FuncEnvironment<'environment> {
    /// Get the information needed to produce Cranelift IR for the given target.
    fn target_config(&self) -> TargetFrontendConfig {
        self.mod_info.config
    }

    /// Get the Cranelift integer type to use for native pointers.
    ///
    /// This returns `I64` for 64-bit architectures and `I32` for 32-bit architectures.
    fn pointer_type(&self) -> ir::Type {
        ir::Type::int(u16::from(self.target_config().pointer_bits())).unwrap()
    }

    /// Get the size of a native pointer, in bytes.
    fn pointer_bytes(&self) -> u8 {
        self.target_config().pointer_bytes()
    }

    // TODO: offsets should be based on the architecture the wasmer was compiled for.
    //      e.g., BoundedSlice.len will be 32-bit (4 bytes) when wasmer is compiled for a 32-bit arch,
    //      however the 32-bit wasmer may be running on 64-bit arch, which means ptr_size here will
    //      be 8 bytes. That will definitely gove the wrong offset values
    fn make_table(&mut self, func: &mut ir::Function, table_index: TableIndex) -> ir::Table {
        assert_eq!(
            table_index.index(),
            0,
            "Only one WebAssembly memory supported"
        );
        let instance = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_size = native_pointer_size();
        // Load value at (instance + TABLES_OFFSET)
        // which is the address of data_pointer.tables
        let base = func.create_global_value(ir::GlobalValueData::Load {
            base: instance,
            offset: Offset32::new(Instance::TABLES_OFFSET as i32),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // Offset based on table_index
        let table_data_offset = table_index.index() as i32 * ptr_size * 2;

        // Load value at the (base + table_data_offset)
        // which is the address of data_pointer.tables[index].data
        let base_gv = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(table_data_offset),
            global_type: native_pointer_type(),
            readonly: false,
        });

        // Load value at the (base + table_data_offset)
        // which is the value of data_pointer.tables[index].len
        let bound_gv = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(table_data_offset + ptr_size as i32),
            global_type: self.pointer_type(),
            readonly: false,
        });

        // Create table based on the data above
        func.create_table(ir::TableData {
            base_gv,
            min_size: Uimm64::new(0),
            bound_gv,
            element_size: Uimm64::new(u64::from(self.pointer_bytes())),
            index_type: I64,
        })
    }

    // TODO: offsets should be based on the architecture the wasmer was compiled for.
    fn make_heap(&mut self, func: &mut ir::Function, memory_index: MemoryIndex) -> ir::Heap {
        debug_assert_eq!(
            memory_index.index(),
            0,
            "Only one WebAssembly memory supported"
        );
        let instance = func.create_global_value(ir::GlobalValueData::VMContext);
        let ptr_size = native_pointer_size();

        // Load value at (instance + MEMORIES_OFFSET)
        // which is the address of data_pointer.memories
        let base = func.create_global_value(ir::GlobalValueData::Load {
            base: instance,
            offset: Offset32::new(Instance::MEMORIES_OFFSET as i32),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // Based on the index provided, we need to know the offset into memories array
        let memory_data_offset = (memory_index.index() * ptr_size as usize) as i32;

        // Load value at the (base + memory_data_offset)
        // which is the address of data_pointer.memories[index].data
        let heap_base = func.create_global_value(ir::GlobalValueData::Load {
            base,
            offset: Offset32::new(memory_data_offset),
            global_type: self.pointer_type(),
            readonly: true,
        });

        // Create table based on the data above
        func.create_heap(ir::HeapData {
            base: heap_base,
            min_size: 0.into(),
            offset_guard_size: Uimm64::new(LinearMemory::DEFAULT_GUARD_SIZE as u64),
            style: ir::HeapStyle::Static {
                bound: Uimm64::new(LinearMemory::DEFAULT_HEAP_SIZE as u64),
            },
            index_type: I32,
        })
    }

    fn make_global(
        &mut self,
        func: &mut ir::Function,
        global_index: GlobalIndex,
    ) -> GlobalVariable {
        let ptr_size = native_pointer_size();

        let instance = func.create_global_value(ir::GlobalValueData::VMContext);

        let globals_base_addr = func.create_global_value(ir::GlobalValueData::Load {
            base: instance,
            offset: Offset32::new(Instance::GLOBALS_OFFSET as i32),
            global_type: self.pointer_type(),
            readonly: false,
        });

        let offset = global_index.index() * ptr_size as usize;
        // let iadd = func.create_global_value(ir::GlobalValueData::IAddImm {
        //     base: globals_base_addr,
        //     offset: Imm64::new(offset),
        //     global_type: native_pointer_type(),
        // });

        GlobalVariable::Memory {
            gv: globals_base_addr,
            offset: (offset as i32).into(),
            ty: self.mod_info.globals[global_index.index()].entity.ty,
        }
    }

    fn make_indirect_sig(&mut self, func: &mut ir::Function, index: SignatureIndex) -> ir::SigRef {
        // A real implementation would probably change the calling convention and add `vmctx` and
        // signature index arguments.
        // func.import_signature(self.mod_info.signatures[index].clone())
        func.import_signature(self.vmctx_sig(index))
    }

    fn make_direct_func(&mut self, func: &mut ir::Function, index: FuncIndex) -> ir::FuncRef {
        let sigidx = self.mod_info.functions[index].entity;
        // A real implementation would probably add a `vmctx` argument.
        // And maybe attempt some signature de-duplication.
        let signature = func.import_signature(self.vmctx_sig(sigidx));
        let name = get_func_name(index);
        func.import_function(ir::ExtFuncData {
            name,
            signature,
            colocated: false,
        })
    }

    fn translate_call_indirect(
        &mut self,
        mut pos: FuncCursor,
        _table_index: TableIndex,
        table: ir::Table,
        _sig_index: SignatureIndex,
        sig_ref: ir::SigRef,
        callee: ir::Value,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Pass the current function's vmctx parameter on to the callee.
        let vmctx = pos
            .func
            .special_param(ir::ArgumentPurpose::VMContext)
            .expect("Missing vmctx parameter");

        // The `callee` value is an index into a table of function pointers.
        // Apparently, that table is stored at absolute address 0 in this dummy environment.
        // TODO: Generate bounds checking code.
        let ptr = native_pointer_type();
        let callee_offset = if ptr == I32 {
            // pos.ins().imul_imm(callee, 4)
            callee
        } else {
            pos.ins().uextend(ptr, callee)
            // let ext = pos.ins().uextend(ptr, callee);
            // pos.ins().imul_imm(ext, 4)
        };
        // let entry_size = native_pointer_size() as i64 * 2;
        // let callee_scaled = pos.ins().imul_imm(callee_offset, entry_size);

        let entry_addr = pos.ins().table_addr(ptr, table, callee_offset, 0);

        let mut mflags = ir::MemFlags::new();
        mflags.set_notrap();
        mflags.set_aligned();
        let func_ptr = pos.ins().load(ptr, mflags, entry_addr, 0);

        pos.ins().trapz(func_ptr, TrapCode::IndirectCallToNull);

        // Build a value list for the indirect call instruction containing the callee, call_args,
        // and the vmctx parameter.
        let mut args = ir::ValueList::default();
        args.push(func_ptr, &mut pos.func.dfg.value_lists);
        args.extend(call_args.iter().cloned(), &mut pos.func.dfg.value_lists);
        args.push(vmctx, &mut pos.func.dfg.value_lists);

        let inst = pos
            .ins()
            .CallIndirect(ir::Opcode::CallIndirect, INVALID, sig_ref, args)
            .0;

        Ok(inst)
    }

    fn translate_call(
        &mut self,
        mut pos: FuncCursor,
        _callee_index: FuncIndex,
        callee: ir::FuncRef,
        call_args: &[ir::Value],
    ) -> WasmResult<ir::Inst> {
        // Pass the current function's vmctx parameter on to the callee.
        let vmctx = pos
            .func
            .special_param(ir::ArgumentPurpose::VMContext)
            .expect("Missing vmctx parameter");

        // println!("POINTER BYTES {}", self.pointer_bytes());
        // println!("POINTER SIZE {}", native_pointer_size());

        // Build a value list for the call instruction containing the call_args and the vmctx
        // parameter.
        let mut args = ir::ValueList::default();
        args.extend(call_args.iter().cloned(), &mut pos.func.dfg.value_lists);
        args.push(vmctx, &mut pos.func.dfg.value_lists);

        Ok(pos.ins().Call(ir::Opcode::Call, INVALID, callee, args).0)
    }

    fn translate_memory_grow(
        &mut self,
        mut pos: FuncCursor,
        memory_index: MemoryIndex,
        _heap: ir::Heap,
        val: ir::Value,
    ) -> WasmResult<ir::Value> {
        debug_assert_eq!(
            memory_index.index(),
            0,
            "non-default memories not supported yet"
        );
        let grow_mem_func = self.mod_info.grow_memory_extfunc.unwrap_or_else(|| {
            let sig_ref = pos.func.import_signature(Signature {
                call_conv: CallConv::SystemV,
                // argument_bytes: None,
                params: vec![
                    // Size
                    AbiParam::new(I32),
                    // Memory index
                    AbiParam::new(I32),
                    // VMContext
                    AbiParam::special(self.pointer_type(), ArgumentPurpose::VMContext),
                ],
                returns: vec![AbiParam::new(I32)],
            });

            pos.func.import_function(ExtFuncData {
                name: ExternalName::testcase("grow_memory"),
                signature: sig_ref,
                colocated: false,
            })
        });

        // self.mod_info.grow_memory_extfunc = Some(grow_mem_func);
        let memory_index_value = pos.ins().iconst(I32, imm64(memory_index.index()));
        let vmctx = pos.func.special_param(ArgumentPurpose::VMContext).unwrap();

        let call_inst = pos
            .ins()
            .call(grow_mem_func, &[val, memory_index_value, vmctx]);
        Ok(*pos.func.dfg.inst_results(call_inst).first().unwrap())
    }

    fn translate_memory_size(
        &mut self,
        mut pos: FuncCursor,
        memory_index: MemoryIndex,
        _heap: ir::Heap,
    ) -> WasmResult<ir::Value> {
        debug_assert_eq!(
            memory_index.index(),
            0,
            "non-default memories not supported yet"
        );
        let cur_mem_func = self.mod_info.current_memory_extfunc.unwrap_or_else(|| {
            let sig_ref = pos.func.import_signature(Signature {
                call_conv: CallConv::SystemV,
                // argument_bytes: None,
                params: vec![
                    // The memory index
                    AbiParam::new(I32),
                    // The vmctx reference
                    AbiParam::special(self.pointer_type(), ArgumentPurpose::VMContext),
                    // AbiParam::special(I64, ArgumentPurpose::VMContext),
                ],
                returns: vec![AbiParam::new(I32)],
            });

            pos.func.import_function(ExtFuncData {
                name: ExternalName::testcase("current_memory"),
                signature: sig_ref,
                colocated: false,
            })
        });

        // self.mod_info.current_memory_extfunc = cur_mem_func;

        let memory_index_value = pos.ins().iconst(I32, imm64(memory_index.index()));
        let vmctx = pos.func.special_param(ArgumentPurpose::VMContext).unwrap();

        let call_inst = pos.ins().call(cur_mem_func, &[memory_index_value, vmctx]);
        Ok(*pos.func.dfg.inst_results(call_inst).first().unwrap())
        // Ok(pos.ins().iconst(I32, -1))
    }

    fn return_mode(&self) -> ReturnMode {
        ReturnMode::NormalReturns
        // ReturnMode::FallthroughReturn
    }
}

impl<'data> ModuleEnvironment<'data> for Module {
    fn target_config(&self) -> TargetFrontendConfig {
        self.info.config
    }

    fn declare_signature(&mut self, sig: &ir::Signature) {
        // OLD
        self.info.signatures.push(sig.clone());

        // // NEW
        // let mut sig = sig.clone();
        // sig.params.push(AbiParam {
        //     value_type: self.native_pointer(),
        //     purpose: ArgumentPurpose::VMContext,
        //     extension: ArgumentExtension::None,
        //     location: ArgumentLoc::Unassigned,
        // });
        // // TODO: Deduplicate signatures.
        // self.info.signatures.push(sig);
    }

    fn get_signature(&self, sig_index: SignatureIndex) -> &ir::Signature {
        &self.info.signatures[sig_index.index()]
    }

    fn declare_func_import(
        &mut self,
        sig_index: SignatureIndex,
        module: &'data str,
        field: &'data str,
    ) {
        assert_eq!(
            self.info.functions.len(),
            self.info.imported_funcs.len(),
            "Imported functions must be declared first"
        );
        self.info
            .functions
            .push(ImportableExportable::new(sig_index, None));
        self.info
            .imported_funcs
            .push((String::from(module), String::from(field)));
    }

    fn get_num_func_imports(&self) -> usize {
        self.info.imported_funcs.len()
    }

    fn declare_func_type(&mut self, sig_index: SignatureIndex) {
        self.info
            .functions
            .push(ImportableExportable::new(sig_index, None));
    }

    fn get_func_type(&self, func_index: FuncIndex) -> SignatureIndex {
        self.info.functions[func_index].entity
    }

    fn declare_global(&mut self, global: Global) {
        self.info
            .globals
            .push(ImportableExportable::new(global, None));
    }

    fn declare_global_import(&mut self, global: Global, module: &'data str, field: &'data str) {
        self.info.globals.push(ImportableExportable::new(
            global,
            Some((String::from(module), String::from(field))),
        ));
    }

    fn get_global(&self, global_index: GlobalIndex) -> &Global {
        &self.info.globals[global_index.index()].entity
    }

    fn declare_table(&mut self, table: Table) {
        self.info
            .tables
            .push(ImportableExportable::new(table, None));
    }

    fn declare_table_import(&mut self, table: Table, module: &'data str, field: &'data str) {
        self.info.tables.push(ImportableExportable::new(
            table,
            Some((String::from(module), String::from(field))),
        ));
    }

    fn declare_table_elements(
        &mut self,
        table_index: TableIndex,
        base: Option<GlobalIndex>,
        offset: usize,
        elements: Vec<FuncIndex>,
    ) {
        self.info.table_elements.push(TableElements {
            table_index,
            base,
            offset,
            elements,
        });
    }

    fn declare_memory(&mut self, memory: Memory) {
        self.info
            .memories
            .push(ImportableExportable::new(memory, None));
    }

    fn declare_memory_import(&mut self, memory: Memory, module: &'data str, field: &'data str) {
        self.info.memories.push(ImportableExportable::new(
            memory,
            Some((String::from(module), String::from(field))),
        ));
    }

    fn declare_data_initialization(
        &mut self,
        memory_index: MemoryIndex,
        base: Option<GlobalIndex>,
        offset: usize,
        data: &'data [u8],
    ) {
        self.info.data_initializers.push(DataInitializer {
            memory_index,
            base,
            offset,
            data: data.to_vec(),
        });
    }

    fn declare_func_export(&mut self, func_index: FuncIndex, name: &'data str) {
        self.info.functions[func_index]
            .export_names
            .push(String::from(name));
        // We add to the exports to have O(1) retrieval
        self.info
            .exports
            .insert(name.to_string(), Export::Function(func_index));
    }

    fn declare_table_export(&mut self, table_index: TableIndex, name: &'data str) {
        self.info.tables[table_index.index()]
            .export_names
            .push(String::from(name));
        // We add to the exports to have O(1) retrieval
        self.info
            .exports
            .insert(name.to_string(), Export::Table(table_index));
    }

    fn declare_memory_export(&mut self, memory_index: MemoryIndex, name: &'data str) {
        self.info.memories[memory_index.index()]
            .export_names
            .push(String::from(name));
        // We add to the exports to have O(1) retrieval
        self.info
            .exports
            .insert(name.to_string(), Export::Memory(memory_index));
    }

    fn declare_global_export(&mut self, global_index: GlobalIndex, name: &'data str) {
        self.info.globals[global_index.index()]
            .export_names
            .push(String::from(name));
        // We add to the exports to have O(1) retrieval
        self.info
            .exports
            .insert(name.to_string(), Export::Global(global_index));
    }

    fn declare_start_func(&mut self, func_index: FuncIndex) {
        debug_assert!(self.info.start_func.is_none());
        self.info.start_func = Some(func_index);
    }

    fn define_function_body(&mut self, body_bytes: &'data [u8]) -> WasmResult<()> {
        let func = {
            let mut func_environ = FuncEnvironment::new(&self.info); // , self.return_mode);
            let func_index =
                FuncIndex::new(self.get_num_func_imports() + self.info.function_bodies.len());
            let name = get_func_name(func_index);
            let sig = func_environ.vmctx_sig(self.get_func_type(func_index));
            let mut func = ir::Function::with_name_signature(name, sig);
            self.trans
                .translate(body_bytes, &mut func, &mut func_environ)?;
            func
        };
        self.func_bytecode_sizes.push(body_bytes.len());
        self.info.function_bodies.push(func);
        Ok(())
    }
}
