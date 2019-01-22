//! The relocation package provide two structures: RelocSink, TrapSink.
//! This structures are used by Cranelift when compiling functions to mark
//! any other calls that this function is doing, so we can "patch" the
//! function addrs in runtime with the functions we need.
use cranelift_codegen::binemit;
pub use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::ir::{self, ExternalName, LibCall, SourceLoc, TrapCode};
use hashbrown::HashMap;
use wasmer_runtime_core::{structures::TypedIndex, types::LocalFuncIndex};

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

#[derive(Debug, Clone, Copy)]
pub enum VmCall {
    LocalStaticMemoryGrow,
    LocalStaticMemorySize,
    ImportedStaticMemoryGrow,
    ImportedStaticMemorySize,
}

/// Specify the type of relocation
#[derive(Debug, Clone)]
pub enum RelocationType {
    Normal(LocalFuncIndex),
    Intrinsic(String),
    LibCall(LibCall),
    VmCall(VmCall),
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
                let target = RelocationType::VmCall(match index {
                    0 => VmCall::LocalStaticMemoryGrow,
                    1 => VmCall::LocalStaticMemorySize,
                    2 => VmCall::ImportedStaticMemoryGrow,
                    3 => VmCall::ImportedStaticMemorySize,
                    _ => unimplemented!(),
                });
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

#[derive(Debug, Clone, Copy)]
pub struct TrapData {
    pub trapcode: TrapCode,
    pub srcloc: SourceLoc,
}

/// Simple implementation of a TrapSink
/// that saves the info for later.
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
    fn trap(&mut self, offset: u32, srcloc: SourceLoc, trapcode: TrapCode) {
        self.trap_datas
            .push((offset as usize, TrapData { trapcode, srcloc }));
    }
}
