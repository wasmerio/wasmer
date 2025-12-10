//! Helpers for generating DWARF LSDA data for Cranelift-compiled functions.
//!
//! The structures and encoding implemented here mirror what LLVM produces for
//! Wasm exception handling so that Wasmer's libunwind personalities can parse
//! the tables without any runtime changes.

use cranelift_codegen::{
    ExceptionContextLoc, FinalizedMachCallSite, FinalizedMachExceptionHandler,
};
use cranelift_entity::EntityRef;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

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

        // TODO: adjust to a single handler -> reflects the reality what we use!!!
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

    if std::env::var_os("WASMER_DEBUG_EH").is_some() {
        eprintln!("[wasmer][eh] call sites: {sites:?}");
    }

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
    let mut bytes = Vec::new();
    bytes.push(DW_EH_PE_OMIT); // lpstart encoding omitted (relative to function start)

    if type_entries.is_empty() {
        bytes.push(DW_EH_PE_OMIT);
    } else {
        bytes.push(DW_EH_PE_ABSPTR);
    }

    if !type_entries.is_empty() {
        let ttype_table_end = 1 // call-site encoding byte
            + uleb128_len(call_site_table_len)
            + call_site_table.len()
            + action_table.bytes.len()
            + type_table_bytes.len();
        write_uleb128(ttype_table_end as u64, &mut bytes);
    }

    bytes.push(DW_EH_PE_UDATA4);
    write_uleb128(call_site_table_len, &mut bytes);
    bytes.extend_from_slice(&call_site_table);
    bytes.extend_from_slice(&action_table.bytes);

    let type_table_offset = bytes.len() as u32;
    bytes.extend_from_slice(&type_table_bytes);

    let mut relocations = Vec::new();
    for reloc in type_table_relocs {
        relocations.push(TagRelocation {
            offset: type_table_offset + reloc.offset,
            tag: reloc.tag,
        });
    }

    Some(FunctionLsdaData { bytes, relocations })
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
/// its LSDA offset inside the section.
pub fn build_lsda_section(
    lsda_data: Vec<Option<FunctionLsdaData>>,
    pointer_bytes: u8,
    tag_offsets: &HashMap<u32, u32>,
    tag_section_index: Option<SectionIndex>,
) -> (Option<CustomSection>, Vec<Option<u32>>) {
    let mut bytes = Vec::new();
    let mut relocations = Vec::new();
    let mut offsets_per_function = Vec::with_capacity(lsda_data.len());
    let debug_lsda = std::env::var_os("WASMER_DEBUG_EH").is_some();

    let pointer_kind = match pointer_bytes {
        4 => RelocationKind::Abs4,
        8 => RelocationKind::Abs8,
        other => panic!("unsupported pointer size {other} for LSDA generation"),
    };

    for (func_idx, data) in lsda_data.into_iter().enumerate() {
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
            if debug_lsda {
                eprintln!(
                    "[wasmer][eh] func #{func_idx} lsda size={} relocations={} bytes={:02x?}",
                    data.bytes.len(),
                    data.relocations.len(),
                    &data.bytes
                );
            }
        } else {
            offsets_per_function.push(None);
            if debug_lsda {
                eprintln!("[wasmer][eh] func #{func_idx} has no LSDA");
            }
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
    let mut bytes = Vec::new();
    let mut first_action_offsets = Vec::new();

    for actions in callsite_actions {
        if actions.is_empty() {
            first_action_offsets.push(None);
        } else {
            let mut last_action_start = 0;
            for (i, &ttype_index) in actions.iter().enumerate() {
                let next_action_start = bytes.len() as i64;
                write_sleb128(ttype_index as i64, &mut bytes);

                if i != 0 {
                    // Make a linked list to the previous action
                    write_sleb128(last_action_start - bytes.len() as i64, &mut bytes);
                } else {
                    write_sleb128(0, &mut bytes);
                }
                last_action_start = next_action_start;
            }
            first_action_offsets.push(Some(last_action_start as u32));
        }
    }

    ActionTable {
        bytes,
        first_action_offsets,
    }
}

fn encode_call_site_table(callsites: &[CallSiteDesc], action_table: &ActionTable) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (idx, site) in callsites.iter().enumerate() {
        write_encoded_offset(site.start, &mut bytes);
        write_encoded_offset(site.len, &mut bytes);
        write_encoded_offset(site.landing_pad, &mut bytes);

        let action = match action_table.first_action_offsets[idx] {
            Some(offset) => offset as u64 + 1,
            None => 0,
        };
        write_uleb128(action, &mut bytes);
    }
    bytes
}

fn write_encoded_offset(val: u32, out: &mut Vec<u8>) {
    // We use DW_EH_PE_udata4 for all offsets.
    out.extend_from_slice(&val.to_le_bytes());
}

fn write_uleb128(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn uleb128_len(mut value: u64) -> usize {
    let mut len = 0;
    loop {
        value >>= 7;
        len += 1;
        if value == 0 {
            break;
        }
    }
    len
}

fn write_sleb128(mut value: i64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        let sign_bit = (byte & 0x40) != 0;
        value >>= 7;
        let done = (value == 0 && !sign_bit) || (value == -1 && sign_bit);
        out.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}

const DW_EH_PE_OMIT: u8 = 0xff;
const DW_EH_PE_ABSPTR: u8 = 0x00;
const DW_EH_PE_UDATA4: u8 = 0x03;
