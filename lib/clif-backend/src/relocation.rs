//! The relocation package provide two structures: RelocSink, TrapSink.
//! This structures are used by Cranelift when compiling functions to mark
//! any other calls that this function is doing, so we can "patch" the
//! function addrs in runtime with the functions we need.
use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, ExternalName, SourceLoc};
use hashbrown::HashMap;
use wasmer_runtime_core::{
    structures::TypedIndex,
    types::{LocalFuncIndex, SigIndex},
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

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone)]
pub enum Reloc {
    Abs8,
    X86PCRel4,
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone)]
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

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Relocation {
    /// The relocation code.
    pub reloc: Reloc,
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
    /// Relocation type.
    pub target: RelocationType,
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum VmCallKind {
    StaticMemoryGrow,
    StaticMemorySize,

    SharedStaticMemoryGrow,
    SharedStaticMemorySize,

    DynamicMemoryGrow,
    DynamicMemorySize,
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum VmCall {
    Local(VmCallKind),
    Import(VmCallKind),
}

/// Specify the type of relocation
#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub enum RelocationType {
    Normal(LocalFuncIndex),
    Intrinsic(String),
    LibCall(LibCall),
    VmCall(VmCall),
    Signature(SigIndex),
}

/// Implementation of a relocation sink that just saves all the information for later
pub struct RelocSink {
    /// Relocations recorded for the function.
    pub relocs: Vec<Relocation>,
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
            _ => unimplemented!("unimplented reloc type: {}", reloc),
        };

        match *name {
            ExternalName::User {
                namespace: 0,
                index,
            } => {
                self.relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target: RelocationType::Normal(LocalFuncIndex::new(index as usize)),
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
                self.relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target,
                });
            }
            ExternalName::TestCase { length, ascii } => {
                let (slice, _) = ascii.split_at(length as usize);
                let name = String::from_utf8(slice.to_vec()).unwrap();
                self.relocs.push(Relocation {
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
                self.relocs.push(Relocation {
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

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy)]
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
    User(u16),
}

/// Implementation of a relocation sink that just saves all the information for later
impl RelocSink {
    pub fn new() -> RelocSink {
        RelocSink { relocs: Vec::new() }
    }
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct TrapData {
    pub trapcode: TrapCode,
    pub srcloc: u32,
}

/// Simple implementation of a TrapSink
/// that saves the info for later.
#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
pub struct TrapSink {
    trap_datas: HashMap<usize, TrapData>,
}

impl TrapSink {
    pub fn new() -> TrapSink {
        TrapSink {
            trap_datas: HashMap::new(),
        }
    }

    pub fn lookup(&self, offset: usize) -> Option<TrapData> {
        self.trap_datas.get(&offset).cloned()
    }

    pub fn drain_local(&mut self, current_func_offset: usize, local: &mut LocalTrapSink) {
        local.trap_datas.drain(..).for_each(|(offset, trap_data)| {
            self.trap_datas
                .insert(current_func_offset + offset, trap_data);
        });
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
