//! The relocation package provide two structures: RelocSink, TrapSink.
//! This structures are used by Cranelift when compiling functions to mark
//! any other calls that this function is doing, so we can "patch" the
//! function addrs in runtime with the functions we need.
use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, ExternalName, LibCall, SourceLoc, TrapCode};
use wasmer_runtime::{structures::TypedIndex, types::LocalFuncIndex};

pub use cranelift_codegen::binemit::Reloc;

#[derive(Debug, Clone)]
pub struct Relocation {
    /// The relocation code.
    pub reloc: binemit::Reloc,
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
    /// Relocation type.
    pub target: RelocationType,
}

/// Specify the type of relocation
#[derive(Debug, Clone)]
pub enum RelocationType {
    Normal(LocalFuncIndex),
    Intrinsic(String),
    LibCall(LibCall),
    StaticGrowMemory,
    StaticCurrentMemory,
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
        unimplemented!();
    }
    fn reloc_external(
        &mut self,
        offset: binemit::CodeOffset,
        reloc: binemit::Reloc,
        name: &ExternalName,
        addend: binemit::Addend,
    ) {
        match *name {
            ExternalName::User {
                namespace: 0,
                index,
            } => {
                self.func_relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target: RelocationType::Normal(LocalFuncIndex::new(index as usize)),
                });
            }
            ExternalName::User {
                namespace: 1,
                index,
            } => {
                let target = match index {
                    0 => RelocationType::StaticGrowMemory,
                    1 => RelocationType::StaticCurrentMemory,
                    _ => unimplemented!(),
                };
                self.func_relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target,
                });
            }
            ExternalName::TestCase { length, ascii } => {
                let (slice, _) = ascii.split_at(length as usize);
                let name = String::from_utf8(slice.to_vec()).unwrap();
                self.func_relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target: RelocationType::Intrinsic(name),
                });
            }
            ExternalName::LibCall(libcall) => {
                let relocation_type = RelocationType::LibCall(libcall);
                self.func_relocs.push(Relocation {
                    reloc,
                    offset,
                    addend,
                    target: relocation_type,
                });
            }
            _ => {
                unimplemented!();
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

/// Implementation of a relocation sink that just saves all the information for later
impl RelocSink {
    pub fn new() -> RelocSink {
        RelocSink {
            func_relocs: Vec::new(),
        }
    }
}

pub struct TrapData {
    pub offset: usize,
    pub code: TrapCode,
}

/// Simple implementation of a TrapSink
/// that saves the info for later.
pub struct TrapSink {
    trap_datas: Vec<TrapData>,
}

impl TrapSink {
    pub fn new() -> TrapSink {
        TrapSink {
            trap_datas: Vec::new(),
        }
    }
}

impl binemit::TrapSink for TrapSink {
    fn trap(&mut self, offset: u32, _: SourceLoc, code: TrapCode) {
        self.trap_datas.push(TrapData {
            offset: offset as usize,
            code,
        });
    }
}
