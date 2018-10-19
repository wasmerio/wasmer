//! The relocation package provide two structures: RelocSink, TrapSink.
//! This structures are used by Cranelift when compiling functions to mark
//! any other calls that this function is doing, so we can "patch" the
//! function addrs in runtime with the functions we need.
use cranelift_codegen::binemit;
use cranelift_codegen::ir::{self, ExternalName, SourceLoc, TrapCode};

pub use cranelift_codegen::binemit::Reloc;
use cranelift_wasm::FuncIndex;

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
    GrowMemory,
    CurrentMemory,
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
                self.func_relocs.push((
                    Relocation {
                        reloc,
                        offset,
                        addend,
                    },
                    RelocationType::Normal(index as _),
                ));
            }
            ExternalName::TestCase { length, ascii } => {
                let (slice, _) = ascii.split_at(length as usize);
                let name = String::from_utf8(slice.to_vec()).unwrap();
                let relocation_type = match name.as_str() {
                    "current_memory" => RelocationType::CurrentMemory,
                    "grow_memory" => RelocationType::GrowMemory,
                    _ => RelocationType::Intrinsic(name),
                };
                self.func_relocs.push((
                    Relocation {
                        reloc,
                        offset,
                        addend,
                    },
                    relocation_type,
                ));
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

impl RelocSink {
    pub fn new() -> RelocSink {
        RelocSink {
            func_relocs: Vec::new(),
        }
    }
}

/// Implementation of a relocation sink that just saves all the information for later
// pub struct RelocSink {
//     /// Relocations recorded for the function.
//     pub func_relocs: Vec<Relocation>,
// }

// impl binemit::RelocSink for RelocSink {
//     fn reloc_ebb(
//         &mut self,
//         _offset: binemit::CodeOffset,
//         _reloc: binemit::Reloc,
//         _ebb_offset: binemit::CodeOffset,
//     ) {
//         // This should use the `offsets` field of `ir::Function`.
//         panic!("ebb headers not yet implemented");
//     }
//     fn reloc_external(
//         &mut self,
//         offset: binemit::CodeOffset,
//         reloc: binemit::Reloc,
//         name: &ExternalName,
//         addend: binemit::Addend,
//     ) {
//         let reloc_target = if let ExternalName::User { namespace, index } = *name {
//             debug_assert!(namespace == 0);
//             RelocationTarget::UserFunc(FuncIndex::new(index as usize))
//         } else if *name == ExternalName::testcase("grow_memory") {
//             RelocationTarget::GrowMemory
//         } else if *name == ExternalName::testcase("current_memory") {
//             RelocationTarget::CurrentMemory
//         } else {
//             panic!("unrecognized external name")
//         };
//         self.func_relocs.push(Relocation {
//             reloc,
//             reloc_target,
//             offset,
//             addend,
//         });
//     }
//     fn reloc_jt(
//         &mut self,
//         _offset: binemit::CodeOffset,
//         _reloc: binemit::Reloc,
//         _jt: ir::JumpTable,
//     ) {
//         panic!("jump tables not yet implemented");
//     }
// }

// impl RelocSink {
//     pub fn new() -> Self {
//         Self {
//             func_relocs: Vec::new(),
//         }
//     }
// }

// /// A record of a relocation to perform.
// #[derive(Debug, Clone)]
// pub struct Relocation {
//     /// The relocation code.
//     pub reloc: binemit::Reloc,
//     /// Relocation target.
//     pub reloc_target: RelocationTarget,
//     /// The offset where to apply the relocation.
//     pub offset: binemit::CodeOffset,
//     /// The addend to add to the relocation value.
//     pub addend: binemit::Addend,
// }

// /// Destination function. Can be either user function or some special one, like grow_memory.
// #[derive(Debug, Copy, Clone)]
// pub enum RelocationTarget {
//     /// The user function index.
//     UserFunc(FuncIndex),
//     /// Function for growing the default memory by the specified amount of pages.
//     GrowMemory,
//     /// Function for query current size of the default linear memory.
//     CurrentMemory,
// }

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
