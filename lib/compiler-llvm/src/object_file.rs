use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use wasmer_compiler::{
    CompileError, CompiledFunctionFrameInfo, CustomSection, CustomSectionProtection,
    CustomSections, FunctionAddressMap, FunctionBody, InstructionAddressMap, Relocation,
    RelocationKind, RelocationTarget, SectionBody, SectionIndex, SourceLoc,
};
use wasmer_types::entity::{PrimaryMap, SecondaryMap};
use wasmer_vm::libcalls::LibCall;

use wasmer_types::entity::entity_impl;
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ElfSectionIndex(u32);
entity_impl!(ElfSectionIndex);
impl ElfSectionIndex {
    pub fn is_undef(&self) -> bool {
        self.as_u32() == goblin::elf::section_header::SHN_UNDEF
    }

    pub fn from_usize(value: usize) -> Result<Self, CompileError> {
        match u32::try_from(value) {
            Err(_) => Err(CompileError::Codegen(format!(
                "elf section index {} does not fit in 32 bits",
                value
            ))),
            Ok(value) => Ok(ElfSectionIndex::from_u32(value)),
        }
    }

    pub fn as_usize(&self) -> usize {
        self.as_u32() as usize
    }
}

fn map_goblin_err(error: goblin::error::Error) -> CompileError {
    CompileError::Codegen(format!("error parsing ELF file: {}", error))
}

pub struct CompiledFunction {
    pub compiled_function: wasmer_compiler::CompiledFunction,
    pub custom_sections: CustomSections,
    pub eh_frame_section_indices: Vec<SectionIndex>,
}

/*
struct ProcedureLinkageTable {
    pub custom_section_id: i32,
    pub plt_targets: ?,
    pub 
}
*/

struct LoadObjectFile<'a> {
    pub contents: &'a [u8],
    pub elf: goblin::elf::Elf<'a>,
    pub libcalls: HashMap<String, LibCall>,
    pub reloc_sections_map: HashMap<ElfSectionIndex, Vec<&'a goblin::elf::reloc::RelocSection<'a>>>,
    pub next_custom_section: u32,
    pub section_indices: HashMap<ElfSectionIndex, SectionIndex>,
}

impl<'a> LoadObjectFile<'a> {
    pub fn new(contents: &'a [u8]) -> Result<LoadObjectFile<'a>, CompileError> {
        let elf = goblin::elf::Elf::parse(&contents).map_err(map_goblin_err)?;

        // TODO: use perfect hash function?
        let mut libcalls = HashMap::new();
        libcalls.insert("wasmer_raise_trap".to_string(), LibCall::RaiseTrap);
        libcalls.insert("truncf".to_string(), LibCall::TruncF32);
        libcalls.insert("trunc".to_string(), LibCall::TruncF64);
        libcalls.insert("ceilf".to_string(), LibCall::CeilF32);
        libcalls.insert("ceil".to_string(), LibCall::CeilF64);
        libcalls.insert("floorf".to_string(), LibCall::FloorF32);
        libcalls.insert("floor".to_string(), LibCall::FloorF64);
        libcalls.insert("nearbyintf".to_string(), LibCall::NearestF32);
        libcalls.insert("nearbyint".to_string(), LibCall::NearestF64);
        libcalls.insert("wasmer_probestack".to_string(), LibCall::Probestack);

        let mut file = LoadObjectFile {
            contents,
            elf,
            libcalls,
            reloc_sections_map: HashMap::new(),
            next_custom_section: 0,
            section_indices: HashMap::new(),
        };

        /*
        file.reloc_sections_map = file.elf.shdr_relocs.iter().fold(
            HashMap::new(),
            |mut map: HashMap<_, Vec<_>>, (section_index, reloc_section)| {
                let target_section = file.elf.section_headers[*section_index].sh_info as usize;
                let target_section = ElfSectionIndex::from_usize(target_section).unwrap();
                map.entry(target_section).or_default().push(reloc_section);
                map
            },
        );
        */

        Ok(file)
    }

    pub fn relocs_for_section(&mut self, elf_section_index: ElfSectionIndex) -> Iter<?> {
        
    }
    
    pub fn init_reloc_sections_map(&mut self) {
        // Build up a mapping from a section to its relocation sections.
        for (section_index, reloc_section) in self.elf.shdr_relocs.iter() {
            let target_section = self.elf.section_headers[*section_index].sh_info as usize;
            let target_section = ElfSectionIndex::from_usize(target_section).unwrap();
            self.reloc_sections_map.entry(target_section).or_default().push(reloc_section);
        }
    }

    pub fn get_section_name(&self, section: &goblin::elf::section_header::SectionHeader) -> Result<Option<&str>, CompileError> {
        if section.sh_name == goblin::elf::section_header::SHN_UNDEF as _ {
            return Ok(None);
        }
        self.elf.strtab.get(section.sh_name).map(|r| r.map_err(map_goblin_err)).map_or(Ok(None), |r| r.map(Some))
    }

    pub fn get_section_bytes(&self, elf_section_index: ElfSectionIndex) -> Vec<u8> {
        let elf_section_index = elf_section_index.as_usize();
        let byte_range = self.elf.section_headers[elf_section_index].file_range();
        self.contents[byte_range.start..byte_range.end].to_vec()
    }

    pub fn new_custom_section_index(&mut self, elf_section_index: ElfSectionIndex) {
        // TODO: use `.unwrap_none()` once that's stabilized.
        debug_assert!(!self.section_indices.contains_key(&elf_section_index));
        self.section_indices.insert(elf_section_index, SectionIndex::from_u32(self.next_custom_section));
        self.next_custom_section += 1;
    }
    
    pub fn get_custom_section_index(&mut self, elf_section_index: ElfSectionIndex) -> SectionIndex {
        *self.section_indices.entry(elf_section_index).or_insert_with(|| {
            let next = SectionIndex::from_u32(self.next_custom_section);
            self.next_custom_section += 1;
            next
        })
    }
}

/*
impl ProcedureLinkageTable {
    pub fn get_relocation_for_fn(&mut self, name: &str) {
        self.
    }
    
    pub fn build_plt(&mut self, &mut ?) {
        
    }
}
 */

pub fn load_object_file<F>(
    contents: &[u8],
    root_section: &str,
    root_section_reloc_target: RelocationTarget,
    mut symbol_name_to_relocation_target: F,
) -> Result<CompiledFunction, CompileError>
where
    F: FnMut(&String) -> Result<Option<RelocationTarget>, CompileError>,
{
    let mut state = LoadObjectFile::new(contents)?;

    let mut visited: HashSet<ElfSectionIndex> = HashSet::new();
    let mut worklist: Vec<ElfSectionIndex> = Vec::new();

    let root_section_index = state.elf
        .section_headers
        .iter()
        .enumerate()
        .filter_map(|(index, section)|
                    match state.get_section_name(section) {
                        Err(err) => Some(Err(err)),
                        Ok(Some(section_name)) if section_name == root_section => Some(Ok(index)),
                        _ => None,
                    })
        .collect::<Result<Vec<_>, _>>()?;
    if root_section_index.len() != 1 {
        return Err(CompileError::Codegen(format!(
            "found {} sections named {}",
            root_section_index.len(),
            root_section
        )));
    }
    let root_section_index = root_section_index[0];
    let root_section_index = ElfSectionIndex::from_usize(root_section_index)?;

    // plt section
    // 48 bb xx xx xx xx xx xx xx xx [xx = relocation to function]
    // ff e3

    // From elf section index to list of Relocations. Although we use a Vec,
    // the order of relocations is not important.
    let mut relocations: HashMap<ElfSectionIndex, Vec<Relocation>> = HashMap::new();

    // Each iteration of this loop pulls a section and the relocations
    // relocations that apply to it. We begin with the ".root_section"
    // section, and then parse all relocation sections that apply to that
    // section. Those relocations may refer to additional sections which we
    // then add to the worklist until we've visited the closure of
    // everything needed to run the code in ".root_section".
    //
    // `worklist` is the list of sections we have yet to visit. It never
    // contains any duplicates or sections we've already visited. `visited`
    // contains all the sections we've ever added to the worklist in a set
    // so that we can quickly check whether a section is new before adding
    // it to worklist. `section_indices` is filled in with all the sections
    // we want to include.
    worklist.push(root_section_index);
    visited.insert(root_section_index);

    // Also add any .eh_frame sections.
    let mut eh_frame_section_indices = vec![];
    // TODO: this constant has been added to goblin, now waiting for release
    const SHT_X86_64_UNWIND: u32 = 0x7000_0001;
    for (index, shdr) in state.elf.section_headers.iter().enumerate() {
        if shdr.sh_type == SHT_X86_64_UNWIND {
            let index = ElfSectionIndex::from_usize(index)?;
            worklist.push(index);
            visited.insert(index);
            eh_frame_section_indices.push(index);
            state.new_custom_section_index(index);
        }
    }

    while let Some(section_index) = worklist.pop() {
        for reloc in state.reloc_sections_map
            .get(&section_index)
            .iter()
            .flat_map(|inner| inner.iter().flat_map(|inner2| inner2.iter()))
        {
            //if reloc.r_
            let kind = match reloc.r_type {
                // TODO: these constants are not per-arch, we'll need to
                // make the whole match per-arch.
                goblin::elf::reloc::R_X86_64_64 => RelocationKind::Abs8,
                goblin::elf::reloc::R_X86_64_PC32 => RelocationKind::X86PCRel4,
                goblin::elf::reloc::R_X86_64_PC64 => RelocationKind::X86PCRel8,
                //goblin::elf::reloc::R_X86_64_PLT32 => ?.get_relocation_for
                _ => {
                    return Err(CompileError::Codegen(format!(
                        "unknown ELF relocation {}",
                        reloc.r_type
                    )));
                }
            };
            let offset = reloc.r_offset as u32;
            let addend = reloc.r_addend.unwrap_or(0);
            let target = reloc.r_sym;
            let elf_target = state.elf.syms.get(target).ok_or_else(|| {
                CompileError::Codegen(format!(
                    "relocation refers to symbol {} past end of symbol table (len={})",
                    target,
                    state.elf.syms.len()
                ))
            })?;
            let elf_target_section = ElfSectionIndex::from_usize(elf_target.st_shndx)?;
            let reloc_target = if elf_target_section == root_section_index {
                root_section_reloc_target
            } else if elf_target.st_type() == goblin::elf::sym::STT_SECTION {
                if visited.insert(elf_target_section) {
                    worklist.push(elf_target_section);
                }
                RelocationTarget::CustomSection((
                    state.get_custom_section_index(elf_target_section),
                    0,
                ))
            } else if elf_target.st_type() == goblin::elf::sym::STT_NOTYPE
                && elf_target_section.is_undef()
            {
                // Not defined in this .o file. Maybe another local function?
                let name = elf_target.st_name;
                let name = state.elf.strtab.get(name).unwrap().unwrap().into();
                if let Some(reloc_target) = symbol_name_to_relocation_target(&name)? {
                    reloc_target
                // Maybe a libcall then?
                } else if let Some(libcall) = state.libcalls.get(&name) {
                    RelocationTarget::LibCall(*libcall)
                } else {
                    unimplemented!("reference to unknown symbol {}", name);
                }
            } else if elf_target.st_type() == goblin::elf::sym::STT_NOTYPE
                && !elf_target_section.is_undef()
            {
                // Reference to specific data in a section.
                if visited.insert(elf_target_section) {
                    worklist.push(elf_target_section);
                }
                RelocationTarget::CustomSection((
                    state.get_custom_section_index(elf_target_section),
                    0,
                )) // TODO
            } else {
                unimplemented!(
                    "unknown relocation {:?} (kind={:?}) with target {:?} (elf_target={:?})",
                    reloc,
                    kind,
                    target,
                    elf_target
                );
            };
            relocations
                .entry(section_index)
                .or_default()
                .push(Relocation {
                    kind,
                    reloc_target,
                    offset,
                    addend,
                });
        }
    }

    let eh_frame_section_indices = eh_frame_section_indices
        .iter()
        .map(|index| {
            state.section_indices.get(index).map_or_else(
                || {
                    Err(CompileError::Codegen(format!(
                        ".eh_frame section with index={:?} was never loaded",
                        index
                    )))
                },
                |idx| Ok(*idx),
            )
        })
        .collect::<Result<Vec<SectionIndex>, _>>()?;

    let mut custom_sections = state.section_indices
        .iter()
        .map(|(elf_section_index, custom_section_index)| {
            (
                custom_section_index,
                CustomSection {
                    protection: CustomSectionProtection::Read,
                    bytes: SectionBody::new_with_vec(state.get_section_bytes(*elf_section_index)),
                    relocations: relocations
                        .remove_entry(elf_section_index)
                        .map_or(vec![], |(_, v)| v),
                },
            )
        })
        .collect::<Vec<_>>();
    custom_sections.sort_unstable_by_key(|a| a.0);
    let custom_sections = custom_sections
        .into_iter()
        .map(|(_, v)| v)
        .collect::<PrimaryMap<SectionIndex, _>>();

    let function_body = FunctionBody {
        body: state.get_section_bytes(root_section_index),
        unwind_info: None,
    };

    let address_map = FunctionAddressMap {
        instructions: vec![InstructionAddressMap {
            srcloc: SourceLoc::default(),
            code_offset: 0,
            code_len: function_body.body.len(),
        }],
        start_srcloc: SourceLoc::default(),
        end_srcloc: SourceLoc::default(),
        body_offset: 0,
        body_len: function_body.body.len(),
    };

    Ok(CompiledFunction {
        compiled_function: wasmer_compiler::CompiledFunction {
            body: function_body,
            jt_offsets: SecondaryMap::new(),
            relocations: relocations
                .remove_entry(&root_section_index)
                .map_or(vec![], |(_, v)| v),
            frame_info: CompiledFunctionFrameInfo {
                address_map,
                traps: vec![],
            },
        },
        custom_sections,
        eh_frame_section_indices,
    })
}
