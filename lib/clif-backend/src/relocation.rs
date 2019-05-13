//! The relocation package provide two structures: RelocSink, TrapSink.
//! This structures are used by Cranelift when compiling functions to mark
//! any other calls that this function is doing, so we can "patch" the
//! function addrs in runtime with the functions we need.
use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, ExternalName, SourceLoc};
use wasmer_runtime_core::{
    structures::TypedIndex,
    types::{FuncIndex, SigIndex},
};

pub mod call_names {
    pub const LOCAL_NAMESPACE: u32 = 1;
    pub const IMPORT_NAMESPACE: u32 = 2;
    pub const SIG_NAMESPACE: u32 = 3;

    pub const STATIC_MEM_GROW: u32 = 0;
    pub const STATIC_MEM_SIZE: u32 = 1;
    pub const SHARED_STATIC_MEM_GROW: u32 = 2;
    pub const SHARED_STATIC_MEM_SIZE: u32 = 3;
    pub const DYNAMIC_MEM_GROW: u32 = 4;
    pub const DYNAMIC_MEM_SIZE: u32 = 5;
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Reloc {
    Abs8,
    X86PCRel4,
    X86CallPCRel4,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub enum LibCall {
    Probestack,
    CeilF32,
    CeilF64,
    FloorF32,
    FloorF64,
    TruncF32,
    TruncF64,
    NearestF32,
    NearestF64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExternalRelocation {
    /// The relocation code.
    pub reloc: Reloc,
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
    /// Relocation type.
    pub target: RelocationType,
}

pub struct LocalRelocation {
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
    /// Relocation type.
    pub target: FuncIndex,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum VmCallKind {
    StaticMemoryGrow,
    StaticMemorySize,

    SharedStaticMemoryGrow,
    SharedStaticMemorySize,

    DynamicMemoryGrow,
    DynamicMemorySize,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum VmCall {
    Local(VmCallKind),
    Import(VmCallKind),
}

/// Specify the type of relocation
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RelocationType {
    Intrinsic(String),
    LibCall(LibCall),
    VmCall(VmCall),
    Signature(SigIndex),
}

/// Implementation of a relocation sink that just saves all the information for later
pub struct RelocSink {
    /// Relocations recorded for the function.
    pub external_relocs: Vec<ExternalRelocation>,
    pub local_relocs: Vec<LocalRelocation>,
}

impl binemit::RelocSink for RelocSink {
    fn reloc_ebb(
        &mut self,
        _offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _ebb_offset: binemit::CodeOffset,
    ) {
        // This should use the `offsets` field of `ir::Function`.
        unimplemented!();
    }
    fn reloc_external(
        &mut self,
        offset: binemit::CodeOffset,
        reloc: binemit::Reloc,
        name: &ExternalName,
        addend: binemit::Addend,
    ) {
        let reloc = match reloc {
            binemit::Reloc::Abs8 => Reloc::Abs8,
            binemit::Reloc::X86PCRel4 => Reloc::X86PCRel4,
            binemit::Reloc::X86CallPCRel4 => Reloc::X86CallPCRel4,
            _ => unimplemented!("unimplented reloc type: {}", reloc),
        };

        match *name {
            ExternalName::User {
                namespace: 0,
                index,
            } => {
                assert_eq!(reloc, Reloc::X86CallPCRel4);
                self.local_relocs.push(LocalRelocation {
                    offset,
                    addend,
                    target: FuncIndex::new(index as usize),
                });
            }
            ExternalName::User { namespace, index } => {
                use self::call_names::*;

                let target = match namespace {
                    LOCAL_NAMESPACE => RelocationType::VmCall(VmCall::Local(match index {
                        STATIC_MEM_GROW => VmCallKind::StaticMemoryGrow,
                        STATIC_MEM_SIZE => VmCallKind::StaticMemorySize,

                        SHARED_STATIC_MEM_GROW => VmCallKind::SharedStaticMemoryGrow,
                        SHARED_STATIC_MEM_SIZE => VmCallKind::SharedStaticMemorySize,

                        DYNAMIC_MEM_GROW => VmCallKind::DynamicMemoryGrow,
                        DYNAMIC_MEM_SIZE => VmCallKind::DynamicMemorySize,
                        _ => unimplemented!(),
                    })),
                    IMPORT_NAMESPACE => RelocationType::VmCall(VmCall::Import(match index {
                        STATIC_MEM_GROW => VmCallKind::StaticMemoryGrow,
                        STATIC_MEM_SIZE => VmCallKind::StaticMemorySize,

                        SHARED_STATIC_MEM_GROW => VmCallKind::SharedStaticMemoryGrow,
                        SHARED_STATIC_MEM_SIZE => VmCallKind::SharedStaticMemorySize,

                        DYNAMIC_MEM_GROW => VmCallKind::DynamicMemoryGrow,
                        DYNAMIC_MEM_SIZE => VmCallKind::DynamicMemorySize,
                        _ => unimplemented!(),
                    })),
                    SIG_NAMESPACE => RelocationType::Signature(SigIndex::new(index as usize)),
                    _ => unimplemented!(),
                };
                self.external_relocs.push(ExternalRelocation {
                    reloc,
                    offset,
                    addend,
                    target,
                });
            }
            ExternalName::TestCase { length, ascii } => {
                let (slice, _) = ascii.split_at(length as usize);
                let name = String::from_utf8(slice.to_vec()).unwrap();
                self.external_relocs.push(ExternalRelocation {
                    reloc,
                    offset,
                    addend,
                    target: RelocationType::Intrinsic(name),
                });
            }
            ExternalName::LibCall(libcall) => {
                let libcall = match libcall {
                    ir::LibCall::CeilF32 => LibCall::CeilF32,
                    ir::LibCall::FloorF32 => LibCall::FloorF32,
                    ir::LibCall::TruncF32 => LibCall::TruncF32,
                    ir::LibCall::NearestF32 => LibCall::NearestF32,
                    ir::LibCall::CeilF64 => LibCall::CeilF64,
                    ir::LibCall::FloorF64 => LibCall::FloorF64,
                    ir::LibCall::TruncF64 => LibCall::TruncF64,
                    ir::LibCall::NearestF64 => LibCall::NearestF64,
                    ir::LibCall::Probestack => LibCall::Probestack,
                    _ => unimplemented!("unimplemented libcall: {}", libcall),
                };
                let relocation_type = RelocationType::LibCall(libcall);
                self.external_relocs.push(ExternalRelocation {
                    reloc,
                    offset,
                    addend,
                    target: relocation_type,
                });
            }
        }
    }
    fn reloc_jt(
        &mut self,
        _offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _jt: ir::JumpTable,
    ) {
        unimplemented!();
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum TrapCode {
    StackOverflow,
    HeapOutOfBounds,
    TableOutOfBounds,
    OutOfBounds,
    IndirectCallToNull,
    BadSignature,
    IntegerOverflow,
    IntegerDivisionByZero,
    BadConversionToInteger,
    Interrupt,
    UnreachableCodeReached,
    User(u16),
}

/// Implementation of a relocation sink that just saves all the information for later
impl RelocSink {
    pub fn new() -> Self {
        Self {
            external_relocs: Vec::new(),
            local_relocs: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct TrapData {
    pub trapcode: TrapCode,
    pub srcloc: u32,
}

/// Simple implementation of a TrapSink
/// that saves the info for later.
#[derive(Serialize, Deserialize)]
pub struct TrapSink {
    trap_datas: Vec<(usize, TrapData)>,
}

impl TrapSink {
    pub fn new() -> TrapSink {
        TrapSink {
            trap_datas: Vec::new(),
        }
    }

    pub fn lookup(&self, offset: usize) -> Option<TrapData> {
        self.trap_datas
            .iter()
            .find(|(trap_offset, _)| *trap_offset == offset)
            .map(|(_, trap_data)| *trap_data)
    }

    pub fn drain_local(&mut self, current_func_offset: usize, local: &mut LocalTrapSink) {
        self.trap_datas.extend(
            local
                .trap_datas
                .drain(..)
                .map(|(offset, trap_data)| (current_func_offset + offset, trap_data)),
        );
    }
}

pub struct LocalTrapSink {
    trap_datas: Vec<(usize, TrapData)>,
}

impl LocalTrapSink {
    pub fn new() -> Self {
        LocalTrapSink { trap_datas: vec![] }
    }
}

impl binemit::TrapSink for LocalTrapSink {
    fn trap(&mut self, offset: u32, srcloc: SourceLoc, trapcode: ir::TrapCode) {
        let trapcode = match trapcode {
            ir::TrapCode::StackOverflow => TrapCode::StackOverflow,
            ir::TrapCode::HeapOutOfBounds => TrapCode::HeapOutOfBounds,
            ir::TrapCode::TableOutOfBounds => TrapCode::TableOutOfBounds,
            ir::TrapCode::OutOfBounds => TrapCode::OutOfBounds,
            ir::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
            ir::TrapCode::BadSignature => TrapCode::BadSignature,
            ir::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
            ir::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            ir::TrapCode::BadConversionToInteger => TrapCode::BadConversionToInteger,
            ir::TrapCode::Interrupt => TrapCode::Interrupt,
            ir::TrapCode::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            ir::TrapCode::User(x) => TrapCode::User(x),
        };

        self.trap_datas.push((
            offset as usize,
            TrapData {
                trapcode,
                srcloc: srcloc.bits(),
            },
        ));
    }
}
