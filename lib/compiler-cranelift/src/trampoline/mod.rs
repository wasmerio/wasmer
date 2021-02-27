#![allow(missing_docs)]

mod dynamic_function;
mod function_call;

pub use self::dynamic_function::make_trampoline_dynamic_function;
pub use self::function_call::make_trampoline_function_call;

pub use cranelift_codegen::print_errors::pretty_error;
pub use cranelift_codegen::Context;
pub use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

pub mod binemit {
    pub use cranelift_codegen::binemit::NullTrapSink;
    pub use cranelift_codegen::binemit::{CodeOffset, NullStackMapSink, TrapSink};

    use cranelift_codegen::{binemit, ir};

    /// We don't expect trampoline compilation to produce any relocations, so
    /// this `RelocSink` just asserts that it doesn't recieve any.
    pub struct TrampolineRelocSink {}

    impl binemit::RelocSink for TrampolineRelocSink {
        fn reloc_external(
            &mut self,
            _offset: binemit::CodeOffset,
            _source_loc: ir::SourceLoc,
            _reloc: binemit::Reloc,
            _name: &ir::ExternalName,
            _addend: binemit::Addend,
        ) {
            panic!("trampoline compilation should not produce external symbol relocs");
        }
        fn reloc_constant(
            &mut self,
            _code_offset: binemit::CodeOffset,
            _reloc: binemit::Reloc,
            _constant_offset: ir::ConstantOffset,
        ) {
            panic!("trampoline compilation should not produce constant relocs");
        }
        fn reloc_jt(
            &mut self,
            _offset: binemit::CodeOffset,
            _reloc: binemit::Reloc,
            _jt: ir::JumpTable,
        ) {
            panic!("trampoline compilation should not produce jump table relocs");
        }
    }
}
