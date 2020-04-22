#![allow(missing_docs)]

// mod host;
mod wasm;

// pub use host::make_host_trampoline;
pub use self::wasm::make_wasm_trampoline;

// TODO: Delete
pub mod ir {
    pub use cranelift_codegen::ir::{
        ExternalName, Function, InstBuilder, MemFlags, StackSlotData, StackSlotKind,
    };
}
pub use cranelift_codegen::print_errors::pretty_error;
pub use cranelift_codegen::Context;
pub use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

pub mod binemit {
    pub use cranelift_codegen::binemit::NullTrapSink;
    pub use cranelift_codegen::binemit::{CodeOffset, NullStackmapSink, TrapSink};

    use cranelift_codegen::{binemit, ir};

    /// We don't expect trampoline compilation to produce any relocations, so
    /// this `RelocSink` just asserts that it doesn't recieve any.
    pub struct TrampolineRelocSink {}

    impl binemit::RelocSink for TrampolineRelocSink {
        fn reloc_block(
            &mut self,
            _offset: binemit::CodeOffset,
            _reloc: binemit::Reloc,
            _block_offset: binemit::CodeOffset,
        ) {
            panic!("trampoline compilation should not produce block relocs");
        }
        fn reloc_external(
            &mut self,
            _offset: binemit::CodeOffset,
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
