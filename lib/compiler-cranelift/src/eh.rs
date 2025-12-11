//! Helpers for generating DWARF LSDA data for Cranelift-compiled functions.
//!
//! The structures and encoding implemented here mirror what LLVM produces for
//! Wasm exception handling so that Wasmer's libunwind personalities can parse
//! the tables without any runtime changes.

use cranelift_codegen::{
    ExceptionContextLoc, FinalizedMachCallSite, FinalizedMachExceptionHandler,
};
use cranelift_entity::EntityRef;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::io::{Cursor, Write};

use wasmer_compiler::types::{
    relocation::{Relocation, RelocationKind, RelocationTarget},
    section::{CustomSection, CustomSectionProtection, SectionBody, SectionIndex},
};

/// Relocation information for an LSDA entry that references a tag constant.
#[derive(Debug, Clone)]
pub struct TagRelocation {
    /// Offset within the LSDA blob where the relocation should be applied.
    pub offset: u32,
    /// The module-local exception tag value.
    pub tag: u32,
}

/// Fully encoded LSDA bytes for a single function, together with pending tag
/// relocations that will be resolved once the global tag section is built.
#[derive(Debug, Clone)]
pub struct FunctionLsdaData {
    pub bytes: Vec<u8>,
    pub relocations: Vec<TagRelocation>,
}

/// Build the LSDA for a single function given the finalized Cranelift
/// call-site metadata.
pub fn build_function_lsda<'a>(
    call_sites: impl Iterator<Item = FinalizedMachCallSite<'a>>,
    function_length: usize,
    pointer_bytes: u8,
) -> Option<FunctionLsdaData> {
    let mut sites = Vec::new();

    for site in call_sites {
        let mut catches = Vec::new();
        let mut landing_pad = None;

        // Out landing pads handle all the tags considered for a call instruction, thus
        // we use the latest landing pad.
        for handler in site.exception_handlers {
            match handler {
                FinalizedMachExceptionHandler::Tag(tag, offset) => {
                    landing_pad = Some(landing_pad.unwrap_or(*offset));
                    catches.push(ActionKind::Tag {
                        tag: u32::try_from(tag.index()).expect("tag index fits in u32"),
                    });
                }
                FinalizedMachExceptionHandler::Default(offset) => {
                    landing_pad = Some(landing_pad.unwrap_or(*offset));
                    catches.push(ActionKind::CatchAll);
                }
                FinalizedMachExceptionHandler::Context(context) => {
                    // Context records are used by Cranelift to thread VMContext
                    // information through the landing pad. We emit the LSDA
                    // regardless of whether we see them; nothing to do here.
                    match context {
                        ExceptionContextLoc::SPOffset(_) | ExceptionContextLoc::GPR(_) => {}
                    }
                }
            }
        }

        if catches.is_empty() {
            continue;
        }

        let landing_pad = landing_pad.expect("landing pad offset set when catches exist");
        let cs_start = site.ret_addr.saturating_sub(1);

        sites.push(CallSiteDesc {
            start: cs_start,
            len: 1,
            landing_pad,
            actions: catches,
        });
    }

    if sites.is_empty() {
        return None;
    }

    // Ensure all instructions in the function are covered by filling gaps with
    // default unwinding behavior (no catch actions).
    let mut current_pos = 0u32;
    let mut filled_sites = Vec::new();

    for site in sites {
        if site.start > current_pos {
            // Gap found: add a default site that covers instructions with no handlers
            filled_sites.push(CallSiteDesc {
                start: current_pos,
                len: site.start - current_pos,
                landing_pad: 0,
                actions: Vec::new(),
            });
        }
        current_pos = site.start + site.len;
        filled_sites.push(site);
    }

    // Cover any remaining instructions at the end of the function
    if current_pos < function_length as u32 {
        filled_sites.push(CallSiteDesc {
            start: current_pos,
            len: function_length as u32 - current_pos,
            landing_pad: 0,
            actions: Vec::new(),
        });
    }

    let sites = filled_sites;

    let mut type_entries = TypeTable::new();
    let mut callsite_actions = Vec::with_capacity(sites.len());

    for site in &sites {
        let mut action_indices = Vec::new();
        for action in &site.actions {
            let index = match action {
                ActionKind::Tag { tag } => type_entries.get_or_insert_tag(*tag),
                ActionKind::CatchAll => type_entries.get_or_insert_catch_all(),
            };
            action_indices.push(index as i32);
        }
        callsite_actions.push(action_indices);
    }

    let action_table = encode_action_table(&callsite_actions);
    let call_site_table = encode_call_site_table(&sites, &action_table);
    let (type_table_bytes, type_table_relocs) = type_entries.encode(pointer_bytes);

    let call_site_table_len = call_site_table.len() as u64;
    let mut writer = Cursor::new(Vec::new());
    writer.write_all(&DW_EH_PE_OMIT.to_le_bytes()).unwrap(); // lpstart encoding omitted (relative to function start)

    if type_entries.is_empty() {
        writer.write_all(&DW_EH_PE_OMIT.to_le_bytes()).unwrap();
    } else {
        writer.write_all(&DW_EH_PE_ABSPTR.to_le_bytes()).unwrap();
    }

    if !type_entries.is_empty() {
        let ttype_table_end = 1 // call-site encoding byte
            + uleb128_len(call_site_table_len)
            + call_site_table.len()
            + action_table.bytes.len()
            + type_table_bytes.len();
        leb128::write::unsigned(&mut writer, ttype_table_end as u64).unwrap();
    }

    writer.write_all(&DW_EH_PE_UDATA4.to_le_bytes()).unwrap();
    leb128::write::unsigned(&mut writer, call_site_table_len).unwrap();
    writer.write_all(&call_site_table).unwrap();
    writer.write_all(&action_table.bytes).unwrap();

    let type_table_offset = writer.position() as u32;
    writer.write_all(&type_table_bytes).unwrap();

    let mut relocations = Vec::new();
    for reloc in type_table_relocs {
        relocations.push(TagRelocation {
            offset: type_table_offset + reloc.offset,
            tag: reloc.tag,
        });
    }

    Some(FunctionLsdaData {
        bytes: writer.into_inner(),
        relocations,
    })
}

/// Build the global tag section and a tag->offset map.
pub fn build_tag_section(
    lsda_data: &[Option<FunctionLsdaData>],
) -> Option<(CustomSection, HashMap<u32, u32>)> {
    let mut unique_tags = HashSet::new();
    for data in lsda_data.iter().flatten() {
        for reloc in &data.relocations {
            unique_tags.insert(reloc.tag);
        }
    }

    if unique_tags.is_empty() {
        return None;
    }

    let mut tags: Vec<u32> = unique_tags.into_iter().collect();
    tags.sort_unstable();

    let mut bytes = Vec::with_capacity(tags.len() * std::mem::size_of::<u32>());
    let mut offsets = HashMap::new();
    for tag in tags {
        let offset = bytes.len() as u32;
        bytes.extend_from_slice(&tag.to_ne_bytes());
        offsets.insert(tag, offset);
    }

    let section = CustomSection {
        protection: CustomSectionProtection::Read,
        alignment: None,
        bytes: SectionBody::new_with_vec(bytes),
        relocations: Vec::new(),
    };

    Some((section, offsets))
}

/// Build the LSDA custom section and record the offset for each function.
///
/// Returns the section (if any) and a vector mapping each function index to
/// its LSDA offset inside the section. Even when utilizing the same landing pad for exception tags,
/// Cranelift generates separate landing pad locations.
/// These locations are essentially small trampolines that redirect to the basic block we established (the EH dispatch block).
///
/// The section can be dumped using the elfutils' readelf tool:
/// ```shell
/// objcopy -I binary -O elf64-x86-64 --rename-section .data=.gcc_except_table,alloc,contents lsda.bin object.o && eu-readelf -w object.o
/// ```
pub fn build_lsda_section(
    lsda_data: Vec<Option<FunctionLsdaData>>,
    pointer_bytes: u8,
    tag_offsets: &HashMap<u32, u32>,
    tag_section_index: Option<SectionIndex>,
) -> (Option<CustomSection>, Vec<Option<u32>>) {
    let mut bytes = Vec::new();
    let mut relocations = Vec::new();
    let mut offsets_per_function = Vec::with_capacity(lsda_data.len());

    let pointer_kind = match pointer_bytes {
        4 => RelocationKind::Abs4,
        8 => RelocationKind::Abs8,
        other => panic!("unsupported pointer size {other} for LSDA generation"),
    };

    for data in lsda_data.into_iter() {
        if let Some(data) = data {
            let base = bytes.len() as u32;
            bytes.extend_from_slice(&data.bytes);

            for reloc in &data.relocations {
                let target_offset = tag_offsets
                    .get(&reloc.tag)
                    .copied()
                    .expect("missing tag offset for relocation");
                relocations.push(Relocation {
                    kind: pointer_kind,
                    reloc_target: RelocationTarget::CustomSection(
                        tag_section_index
                            .expect("tag section index must exist when relocations are present"),
                    ),
                    offset: base + reloc.offset,
                    addend: target_offset as i64,
                });
            }

            offsets_per_function.push(Some(base));
        } else {
            offsets_per_function.push(None);
        }
    }

    if bytes.is_empty() {
        (None, offsets_per_function)
    } else {
        (
            Some(CustomSection {
                protection: CustomSectionProtection::Read,
                alignment: None,
                bytes: SectionBody::new_with_vec(bytes),
                relocations,
            }),
            offsets_per_function,
        )
    }
}

// === internal helpers ===

#[derive(Debug)]
struct CallSiteDesc {
    start: u32,
    len: u32,
    landing_pad: u32,
    actions: Vec<ActionKind>,
}

#[derive(Debug)]
enum ActionKind {
    Tag { tag: u32 },
    CatchAll,
}

#[derive(Debug)]
struct TypeTable {
    entries: Vec<TypeEntry>,
    index_map: HashMap<TypeKey, usize>,
}

impl TypeTable {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            index_map: HashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn get_or_insert_tag(&mut self, tag: u32) -> usize {
        let key = TypeKey::Tag(tag);
        if let Some(idx) = self.index_map.get(&key) {
            *idx
        } else {
            let idx = self.entries.len() + 1;
            self.entries.push(TypeEntry::Tag { tag });
            self.index_map.insert(key, idx);
            idx
        }
    }

    fn get_or_insert_catch_all(&mut self) -> usize {
        let key = TypeKey::CatchAll;
        if let Some(idx) = self.index_map.get(&key) {
            *idx
        } else {
            let idx = self.entries.len() + 1;
            self.entries.push(TypeEntry::CatchAll);
            self.index_map.insert(key, idx);
            idx
        }
    }

    fn encode(&self, pointer_bytes: u8) -> (Vec<u8>, Vec<TagRelocation>) {
        let mut bytes = Vec::with_capacity(self.entries.len() * pointer_bytes as usize);
        let mut relocations = Vec::new();

        for entry in self.entries.iter().rev() {
            let offset = bytes.len() as u32;
            match entry {
                TypeEntry::Tag { tag } => {
                    bytes.extend_from_slice(&vec![0; pointer_bytes as usize]);
                    relocations.push(TagRelocation { offset, tag: *tag });
                }
                TypeEntry::CatchAll => {
                    bytes.extend_from_slice(&vec![0; pointer_bytes as usize]);
                }
            }
        }

        (bytes, relocations)
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum TypeKey {
    Tag(u32),
    CatchAll,
}

#[derive(Debug)]
enum TypeEntry {
    Tag { tag: u32 },
    CatchAll,
}

struct ActionTable {
    bytes: Vec<u8>,
    first_action_offsets: Vec<Option<u32>>,
}

fn encode_action_table(callsite_actions: &[Vec<i32>]) -> ActionTable {
    let mut writer = Cursor::new(Vec::new());
    let mut first_action_offsets = Vec::new();

    let mut cache = HashMap::new();

    for actions in callsite_actions {
        if actions.is_empty() {
            first_action_offsets.push(None);
        } else {
            match cache.entry(actions.clone()) {
                Entry::Occupied(entry) => {
                    first_action_offsets.push(Some(*entry.get()));
                }
                Entry::Vacant(entry) => {
                    let mut last_action_start = 0;
                    for (i, &ttype_index) in actions.iter().enumerate() {
                        let next_action_start = writer.position();
                        leb128::write::signed(&mut writer, ttype_index as i64)
                            .expect("leb128 write failed");

                        if i != 0 {
                            // Make a linked list to the previous action
                            let displacement = last_action_start - writer.position() as i64;
                            leb128::write::signed(&mut writer, displacement)
                                .expect("leb128 write failed");
                        } else {
                            leb128::write::signed(&mut writer, 0).expect("leb128 write failed");
                        }
                        last_action_start = next_action_start as i64;
                    }
                    let last_action_start = last_action_start as u32;
                    entry.insert(last_action_start);
                    first_action_offsets.push(Some(last_action_start));
                }
            }
        }
    }

    ActionTable {
        bytes: writer.into_inner(),
        first_action_offsets,
    }
}

fn encode_call_site_table(callsites: &[CallSiteDesc], action_table: &ActionTable) -> Vec<u8> {
    let mut writer = Cursor::new(Vec::new());
    for (idx, site) in callsites.iter().enumerate() {
        write_encoded_offset(site.start, &mut writer);
        write_encoded_offset(site.len, &mut writer);
        write_encoded_offset(site.landing_pad, &mut writer);

        let action = match action_table.first_action_offsets[idx] {
            Some(offset) => offset as u64 + 1,
            None => 0,
        };
        leb128::write::unsigned(&mut writer, action).expect("leb128 write failed");
    }
    writer.into_inner()
}

fn write_encoded_offset(val: u32, out: &mut impl Write) {
    // We use DW_EH_PE_udata4 for all offsets.
    out.write_all(&val.to_le_bytes())
        .expect("write to buffer failed")
}

fn uleb128_len(value: u64) -> usize {
    let mut cursor = Cursor::new([0u8; 10]);
    leb128::write::unsigned(&mut cursor, value).unwrap()
}

const DW_EH_PE_OMIT: u8 = 0xff;
const DW_EH_PE_ABSPTR: u8 = 0x00;
const DW_EH_PE_UDATA4: u8 = 0x03;
