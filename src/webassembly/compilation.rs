//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module.
use cranelift_codegen::binemit;
use cranelift_codegen::ir;
use cranelift_codegen::ir::ExternalName;
use cranelift_codegen::isa;
use cranelift_codegen::Context;
use cranelift_entity::{EntityRef, PrimaryMap};
use cranelift_wasm::{DefinedFuncIndex, FuncIndex, FuncTranslator};
use region::protect;
use region::Protection;

use super::environ::{get_func_name, ModuleTranslation};

pub fn protect_codebuf(code_buf: &Vec<u8>) -> Result<(), String> {
    match unsafe {
        protect(
            code_buf.as_ptr(),
            code_buf.len(),
            Protection::ReadWriteExecute,
        )
    } {
        Err(err) => {
            return Err(format!(
                "failed to give executable permission to code: {}",
                err
            ))
        },
        Ok(()) => Ok(()),
    }
}

/// The result of compiling a WebAssemby module's functions.
#[derive(Debug)]
pub struct LazyFunction {
}

#[derive(Debug)]
pub struct Compilation {
    /// Compiled machine code for the function bodies.
    pub lazy_functions: PrimaryMap<DefinedFuncIndex, LazyFunction>,
    pub functions: PrimaryMap<DefinedFuncIndex, Vec<u8>>,
}

impl Compilation {
    /// Allocates the compilation result with the given function bodies.
    pub fn new(functions: PrimaryMap<DefinedFuncIndex, Vec<u8>>, lazy_functions: PrimaryMap<DefinedFuncIndex, LazyFunction>) -> Self {
        Self { lazy_functions, functions }
    }
}

/// Implementation of a relocation sink that just saves all the information for later
pub struct RelocSink {
    /// Relocations recorded for the function.
    pub func_relocs: Vec<Relocation>,
}

impl binemit::RelocSink for RelocSink {
    fn reloc_ebb(
        &mut self,
        _offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _ebb_offset: binemit::CodeOffset,
    ) {
        // This should use the `offsets` field of `ir::Function`.
        panic!("ebb headers not yet implemented");
    }
    fn reloc_external(
        &mut self,
        offset: binemit::CodeOffset,
        reloc: binemit::Reloc,
        name: &ExternalName,
        addend: binemit::Addend,
    ) {
        // let reloc_target = if let ExternalName::User { namespace, index } = *name {
        //     debug_assert!(namespace == 0);
        //     RelocationTarget::UserFunc(FuncIndex::new(index as usize))
        // } else if *name == ExternalName::testcase("grow_memory") {
        //     RelocationTarget::GrowMemory
        // } else if *name == ExternalName::testcase("current_memory") {
        //     RelocationTarget::CurrentMemory
        // } else {
        //     panic!("unrecognized external name")
        // };
        // self.func_relocs.push(Relocation {
        //     reloc,
        //     reloc_target,
        //     offset,
        //     addend,
        // });
    }
    fn reloc_jt(
        &mut self,
        _offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _jt: ir::JumpTable,
    ) {
        panic!("jump tables not yet implemented");
    }
}

impl RelocSink {
    fn new() -> Self {
        Self {
            func_relocs: Vec::new(),
        }
    }
}

/// A record of a relocation to perform.
#[derive(Debug, Clone)]
pub struct Relocation {
    /// The relocation code.
    pub reloc: binemit::Reloc,
    /// Relocation target.
    pub reloc_target: RelocationTarget,
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
}

/// Destination function. Can be either user function or some special one, like grow_memory.
#[derive(Debug, Copy, Clone)]
pub enum RelocationTarget {
    /// The user function index.
    UserFunc(FuncIndex),
    /// Function for growing the default memory by the specified amount of pages.
    GrowMemory,
    /// Function for query current size of the default linear memory.
    CurrentMemory,
}

/// Relocations to apply to function bodies.
pub type Relocations = PrimaryMap<DefinedFuncIndex, Vec<Relocation>>;

/// Compile the module, producing a compilation result with associated
/// relocations.
pub fn compile_module<'data, 'module>(
    translation: &ModuleTranslation<'data, 'module>,
    isa: &isa::TargetIsa,
) -> Result<(Compilation, Relocations), String> {
    println!("compile_module::1");
    let mut functions = PrimaryMap::new();
    let mut relocations = PrimaryMap::new();
    let mut lazy_functions = PrimaryMap::new();
    println!("compile_module::2");
    for (i, input) in translation.lazy.function_body_inputs.iter() {
        // println!("compile_module::{:?}::3", i);
        let func_index = translation.module.func_index(i);
        let mut context = Context::new();
        // println!("compile_module::{:?}::4", i);
        context.func.name = get_func_name(func_index);
        context.func.signature =
            translation.module.signatures[translation.module.functions[func_index]].clone();

        let mut trans = FuncTranslator::new();
        // println!("compile_module::{:?}::5", i);

        trans
            .translate(input, &mut context.func, &mut translation.func_env())
            .map_err(|e| e.to_string())?;
        // println!("compile_module::{:?}::6", i);

        let mut code_buf: Vec<u8> = Vec::new();
        let mut reloc_sink = RelocSink::new();
        let mut trap_sink = binemit::NullTrapSink {};

        // println!("compile_module::{:?}::7", i);
        context
            .compile_and_emit(isa, &mut code_buf, &mut reloc_sink, &mut trap_sink)
            .map_err(|e| e.to_string())?;
        protect_codebuf(&code_buf)?;

        // println!("compile_module::{:?}::8", i);

        functions.push(code_buf);
        relocations.push(reloc_sink.func_relocs);
    }
    Ok((Compilation::new(functions, lazy_functions), relocations))
}
