//! Support for compiling with Cranelift.

use crate::translator::{irlibcall_to_libcall, irreloc_to_relocationkind};
use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, ExternalName};
use cranelift_entity::EntityRef as CraneliftEntityRef;
use wasmer_compiler::{JumpTable, Relocation, RelocationTarget, TrapInformation};
use wasmer_types::entity::EntityRef;
use wasmer_types::{FunctionIndex, LocalFunctionIndex};
use wasmer_vm::{ModuleInfo, TrapCode};

/// Implementation of a relocation sink that just saves all the information for later
pub(crate) struct RelocSink<'a> {
    module: &'a ModuleInfo,

    /// Current function index.
    local_func_index: LocalFunctionIndex,

    /// Relocations recorded for the function.
    pub func_relocs: Vec<Relocation>,
}

impl<'a> binemit::RelocSink for RelocSink<'a> {
    fn reloc_external(
        &mut self,
        offset: binemit::CodeOffset,
        _source_loc: ir::SourceLoc,
        reloc: binemit::Reloc,
        name: &ExternalName,
        addend: binemit::Addend,
    ) {
        let reloc_target = if let ExternalName::User { namespace, index } = *name {
            debug_assert_eq!(namespace, 0);
            RelocationTarget::LocalFunc(
                self.module
                    .local_func_index(FunctionIndex::from_u32(index))
                    .expect("The provided function should be local"),
            )
        } else if let ExternalName::LibCall(libcall) = *name {
            RelocationTarget::LibCall(irlibcall_to_libcall(libcall))
        } else {
            panic!("unrecognized external name")
        };
        self.func_relocs.push(Relocation {
            kind: irreloc_to_relocationkind(reloc),
            reloc_target,
            offset,
            addend,
        });
    }

    fn reloc_constant(
        &mut self,
        _code_offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _constant_offset: ir::ConstantOffset,
    ) {
        // Do nothing for now: cranelift emits constant data after the function code and also emits
        // function code with correct relative offsets to the constant data.
    }

    fn reloc_jt(&mut self, offset: binemit::CodeOffset, reloc: binemit::Reloc, jt: ir::JumpTable) {
        self.func_relocs.push(Relocation {
            kind: irreloc_to_relocationkind(reloc),
            reloc_target: RelocationTarget::JumpTable(
                self.local_func_index,
                JumpTable::new(jt.index()),
            ),
            offset,
            addend: 0,
        });
    }
}

impl<'a> RelocSink<'a> {
    /// Return a new `RelocSink` instance.
    pub fn new(module: &'a ModuleInfo, func_index: FunctionIndex) -> Self {
        let local_func_index = module
            .local_func_index(func_index)
            .expect("The provided function should be local");
        Self {
            module,
            local_func_index,
            func_relocs: Vec::new(),
        }
    }
}

pub(crate) struct TrapSink {
    pub traps: Vec<TrapInformation>,
}

impl TrapSink {
    pub fn new() -> Self {
        Self { traps: Vec::new() }
    }
}

impl binemit::TrapSink for TrapSink {
    fn trap(
        &mut self,
        code_offset: binemit::CodeOffset,
        _source_loc: ir::SourceLoc,
        trap_code: ir::TrapCode,
    ) {
        self.traps.push(TrapInformation {
            code_offset,
            // TODO: Translate properly environment Trapcode into cranelift IR
            trap_code: translate_ir_trapcode(trap_code),
        });
    }
}

/// Translates the Cranelift IR TrapCode into generic Trap Code
fn translate_ir_trapcode(trap: ir::TrapCode) -> TrapCode {
    match trap {
        ir::TrapCode::StackOverflow => TrapCode::StackOverflow,
        ir::TrapCode::HeapOutOfBounds => TrapCode::HeapAccessOutOfBounds,
        ir::TrapCode::HeapMisaligned => TrapCode::HeapMisaligned,
        ir::TrapCode::TableOutOfBounds => TrapCode::TableAccessOutOfBounds,
        ir::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
        ir::TrapCode::BadSignature => TrapCode::BadSignature,
        ir::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
        ir::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
        ir::TrapCode::BadConversionToInteger => TrapCode::BadConversionToInteger,
        ir::TrapCode::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
        ir::TrapCode::Interrupt => unimplemented!("Interrupts not supported"),
        ir::TrapCode::User(_user_code) => unimplemented!("User trap code not supported"),
        // ir::TrapCode::Interrupt => TrapCode::Interrupt,
        // ir::TrapCode::User(user_code) => TrapCode::User(user_code),
    }
}
