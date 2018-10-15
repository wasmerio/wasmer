use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, TrapCode, SourceLoc, ExternalName};

#[derive(Debug)]
pub struct Relocation {
    /// The relocation code.
    pub reloc: binemit::Reloc,
    /// The offset where to apply the relocation.
    pub offset: binemit::CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: binemit::Addend,
}

/// Specify the type of relocation
#[derive(Debug)]
pub enum RelocationType {
    Normal(u32),
    Intrinsic(String),
}


/// Implementation of a relocation sink that just saves all the information for later
pub struct RelocSink {
    // func: &'func ir::Function,
    /// Relocations recorded for the function.
    pub func_relocs: Vec<(Relocation, RelocationType)>,
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
                self.func_relocs.push(
                    (
                        Relocation {
                            reloc,
                            offset,
                            addend,
                        },
                        RelocationType::Normal(index as _),
                    )
                );
            },
            ExternalName::TestCase {
                length,
                ascii,
            } => {
                let (slice, _) = ascii.split_at(length as usize);
                let name = String::from_utf8(slice.to_vec()).unwrap();

                self.func_relocs.push(
                    (
                        Relocation {
                            reloc,
                            offset,
                            addend,
                        },
                        RelocationType::Intrinsic(name),
                    )
                );
            },
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
    current_func_offset: usize,
    trap_datas: Vec<TrapData>,
}

impl TrapSink {
    pub fn new(current_func_offset: usize) -> TrapSink {
        TrapSink {
            current_func_offset,
            trap_datas: Vec::new(),
        }
    }
}

impl binemit::TrapSink for TrapSink {
    fn trap(&mut self, offset: u32, _: SourceLoc, code: TrapCode) {
        self.trap_datas.push(TrapData {
            offset: self.current_func_offset + offset as usize,
            code,
        });
    }
}
