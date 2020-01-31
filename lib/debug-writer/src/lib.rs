// TODO: add attribution to LLVM for data definitions and WasmTime for code structure
use std::str::FromStr;
use std::ptr;
use std::ffi::c_void;

mod read_debug_info;
mod write_debug_info;
mod gc;
mod transform;

pub use crate::read_debug_info::{read_debug_info, DebugInfoData, WasmFileInfo};
pub use crate::write_debug_info::{emit_dwarf, ResolvedSymbol, SymbolResolver};
use crate::transform::WasmTypesDieRefs;

use target_lexicon::{Triple, Architecture, Vendor, OperatingSystem, Environment, BinaryFormat};
use gimli::write::{self, DwarfUnit, Sections, Address, RangeList, EndianVec, AttributeValue, Range};

use wasmer_runtime_core::{module::ModuleInfo, state::CodeVersion};

/// Triple of x86_64 GNU/Linux
const X86_64_GNU_LINUX: Triple = Triple {
    architecture: Architecture::X86_64,
    vendor: Vendor::Unknown,
    operating_system: OperatingSystem::Linux,
    environment: Environment::Gnu,
    binary_format: BinaryFormat::Elf,
};

/// Triple of x86_64 OSX
const X86_64_OSX: Triple = Triple {
    architecture: Architecture::X86_64,
    vendor: Vendor::Apple,
    operating_system: OperatingSystem::Darwin,
    environment: Environment::Unknown,
    binary_format: BinaryFormat::Macho,
};

/// Triple of x86_64 Windows
const X86_64_WINDOWS: Triple = Triple {
    architecture: Architecture::X86_64,
    vendor: Vendor::Pc,
    operating_system: OperatingSystem::Windows,
    environment: Environment::Msvc,
    binary_format: BinaryFormat::Coff,
};

// this code also from WasmTime
// TODO: attribute
struct ImageRelocResolver<'a> {
    func_offsets: &'a Vec<u64>,
}

// this code also from WasmTime
// TODO: attribute
impl<'a> SymbolResolver for ImageRelocResolver<'a> {
    fn resolve_symbol(&self, symbol: usize, addend: i64) -> ResolvedSymbol {
        let func_start = self.func_offsets[symbol];
        ResolvedSymbol::PhysicalAddress(func_start + addend as u64)
    }
}

// the structure of this function and some of its details come from WasmTime
// TODO: attribute
pub fn generate_dwarf(module_info: &ModuleInfo, debug_info_data: &DebugInfoData, code_version: &CodeVersion, platform: Triple) -> Result<Vec<u8>, String> {
    let func_offsets = unimplemented!();
    let resolver = ImageRelocResolver { func_offsets };
    // copied from https://docs.rs/gimli/0.20.0/gimli/write/index.html ; TODO: review these values
    let processed_dwarf = reprocess_dwarf(module_info, debug_info_data, code_version, platform).ok_or_else(|| "Failed to reprocess Wasm's dwarf".to_string())?;
    let encoding = gimli::Encoding {
        format: gimli::Format::Dwarf32,
        version: 3,
        address_size: 8,
    };
    let mut dwarf = DwarfUnit::new(encoding);
    // TODO: figure out what range is (from example)
    let range_list = RangeList(vec![Range::StartLength {
        begin: Address::Constant(0x100),
        length: 42,
    }]);
    let range_list_id = dwarf.unit.ranges.add(range_list);
    let root = dwarf.unit.root();
    dwarf.unit.get_mut(root).set(
        gimli::DW_AT_ranges,
        AttributeValue::RangeListRef(range_list_id),
    );
    let mut string_table = write::StringTable::default();
    let mut line_string_table = write::LineStringTable::default();

    let mut obj = faerie::Artifact::new(platform, String::from("module"));

    let mut sections = Sections::new(EndianVec::new(gimli::LittleEndian));
    // Finally, write the DWARF data to the sections.
    dwarf.write(&mut sections).map_err(|e| e.to_string())?;
    emit_dwarf(&mut obj, dwarf, &resolver);
    sections.for_each(|id, data| {
        // Here you can add the data to the output object file.
        Ok(())
    });

    obj.emit_as(BinaryFormat::Elf).expect("TODO");
    // We want to populate DwarfUnit::line_str_table with WAT probably
    // and set up the string table with things like function signatures in WAT, function names, etc

    // NOTES from DWARF spec:
    // http://dwarfstd.org/doc/DWARF5.pdf
    // - `DIE`s form the core of dwarf and live in .debug_info
    // - the tags can get fairly specific, it looks like we'll just need a mapping
    //   from object code to a bunch of tags and ranges? created with the Wasm
    //   data for extra info about types, etc.
    // - debug info can live in a separate object file (that's what we'll do here)
    // - attribute types are unique per DIE (lots of info here (like is tail call,
    //   return addr, etc.)
    // - DW_AT_language: WebAssembly :bonjour:
    // - `DW_AT_linkage_name` function namespaces? (later described as the raw, mangled name)
    //   `DW_AT_name` function name?
    // - `DW_AT_location` where in the code it is
    // - `DW_AT_main_subprogram` where to start from
    // - `DW_AT_producer`: wasmer
    // - `DW_AT_recursive` -- is this mandatory?  what is it used for? TODO: find out
    // - `DW_AT_signature` -- can we use wasm type signature info here? TODO:
    // - `DIE`s form a graph/tree though a tree-like graph when it is a graph, docs say
    //   this is how structs and relationship of code blocks is represented.
    // - when serialized the tree is in post-fix order (probably not important for our
    //   purposes but mildly interesting)
    // - we'll need pointer sizer and platform information
    // - dwarf executes a typed stack-machine to compute the locations of things
    // - lots of neat info about the dwarf stack machine skipping for now because I
    //   think gimli exposes a higher-level interface (if not, I'll add notes here
    //   or further down about it)
    // - can use dwarf expressions/dynamically computing things to handle things like
    //   a tiering JIT?
    // - location lists are needed for things that aren't lexically scoped, otherwise
    //   single location descriptions (dwarf expressions) are sufficient
    // - I wonder what this means in the context of spilling registers... do we have
    //   to create dwarf expressions that can handle that?
    // - `DW_AT_artificial` is used to tag `DIE` that didn't come directly from the code
    // - `DW_AT_declaration` for function/etc declarations at the top of the wasm module,
    //    see section 2.13.2 for how to connect the definiton and the declaration
    // - `DW_AT_decl_line`, `DW_AT_decl_column` refer to the exact location in the source
    //    file, so presumably we include the entire source file in one of the sections?
    //    or perhaps that's purely for human consumption.
    // - `DW_AT_ranges` is for non-contiguous ranges of address and,
    //    `DW_AT_low_pc` and `DW_AT_high_pc` are good for continuous
    //    `DW_AT_low_pc` alone can work for a single address, but we can probably not
    //    worry about that for now.  These attribtues associate machine code with the DIE
    // - 
    
    match platform {
        X86_64_GNU_LINUX => unimplemented!("in progress"),
        X86_64_OSX => unimplemented!("in progress"),
        X86_64_WINDOWS => unimplemented!("in progress"),
        _ => return Err(format!("Debug output for the platform {} is not yet supported", platform)),
    }
    Ok(vec![])
}

// converts existing dwarf into a usable form with metadata from the JIT
fn reprocess_dwarf(module_info: &ModuleInfo, debug_info_data: &DebugInfoData, code_version: &CodeVersion, platform: Triple) -> Option<write::Dwarf> {
    None
}

// black box, needs some kind of input, some kind of processing
// and returns a bunch of bytes we can give to GDB
//
// where is this documented?
// we need to pass in target triple, isa config, memories/pointers to memories, ranges of where things are,
// and info like function names
pub fn generate_debug_sections_image() -> Option<Vec<u8>> {
    None
}

// do it

// this code copied from WasmTime, TODO: give attribution


// The `emit_wasm_types` function is a derative work of code in WasmTime:
// TODO: update attributions file and/or do clean reimplementation of this logic
//
//   Copyright 2019 WasmTime Project Developers
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//       http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.
fn emit_wasm_types(unit: &mut write::Unit, root_id: write::UnitEntryId, string_table: &mut write::StringTable) -> WasmTypesDieRefs {
    macro_rules! def_type {
        ($id:literal, $size:literal, $enc:path) => {{
            let die_id = unit.add(root_id, gimli::DW_TAG_base_type);
            let die = unit.get_mut(die_id);
            die.set(
                gimli::DW_AT_name,
                write::AttributeValue::StringRef(string_table.add($id)),
            );
            die.set(gimli::DW_AT_byte_size, write::AttributeValue::Data1($size));
            die.set(gimli::DW_AT_encoding, write::AttributeValue::Encoding($enc));
            die_id
        }};
    }
    let vmctx_id = {
        // TODO: get memory_offset
        let memory_offset = 0;
        let vmctx_die_id = unit.add(root_id, gimli::DW_TAG_structure_type);
        let vmctx_die = unit.get_mut(vmctx_die_id);
        vmctx_die.set(
            gimli::DW_AT_name,
            write::AttributeValue::StringRef(string_table.add("WasmerVMContext")),
        );
        vmctx_die.set(
            gimli::DW_AT_byte_size,
            write::AttributeValue::Data4(memory_offset as u32 + 8),
        );
        let vmctx_ptr_id = unit.add(root_id, gimli::DW_TAG_pointer_type);
        let vmctx_ptr_die = unit.get_mut(vmctx_ptr_id);
        vmctx_ptr_die.set(
            gimli::DW_AT_name,
            write::AttributeValue::StringRef(string_table.add("WasmerVMContext*")),
        );
        vmctx_ptr_die.set(
            gimli::DW_AT_type,
            write::AttributeValue::ThisUnitEntryRef(vmctx_die_id),
        );

        vmctx_ptr_id
    };

    let i32_id = def_type!("i32", 4, gimli::DW_ATE_signed);
    let i64_id = def_type!("i64", 8, gimli::DW_ATE_signed);
    let i128_id = def_type!("i128", 16, gimli::DW_ATE_signed);
    let f32_id = def_type!("f32", 4, gimli::DW_ATE_float);
    let f64_id = def_type!("f64", 8, gimli::DW_ATE_float);

    WasmTypesDieRefs {
        vmctx: vmctx_id,
        i32: i32_id,
        i64: i64_id,
        i128: i128_id,
        f32: f32_id,
        f64: f64_id,
    }
}


// =============================================================================
// LLDB hook magic:
// see lldb/packages/Python/lldbsuite/test/functionalities/jitloader_gdb in
// llvm repo for example
//
// see also https://sourceware.org/gdb/current/onlinedocs/gdb.html#JIT-Interface

#[inline(never)]
pub extern "C" fn __jit_debug_register_code() {
    
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(u32)]
pub enum JITAction { JIT_NOACTION = 0, JIT_REGISTER_FN = 1, JIT_UNREGISTER_FN = 2 }

#[no_mangle]
#[repr(C)]
pub struct JITCodeEntry {
    next: *mut JITCodeEntry,
    prev: *mut JITCodeEntry,
    // TODO: use CStr here?
    symfile_addr: *const u8,
    symfile_size: u64,
}

impl Default for JITCodeEntry {
    fn default() -> Self {
        Self {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            symfile_addr: ptr::null(),
            symfile_size: 0,
        }
    }
}

#[no_mangle]
#[repr(C)]
pub struct JitDebugDescriptor {
    version: u32,
    action_flag: u32,
    relevant_entry: *mut JITCodeEntry,
    first_entry: *mut JITCodeEntry,
}

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static mut __jit_debug_descriptor: JitDebugDescriptor = JitDebugDescriptor {
    version: 1,
    action_flag: JITAction::JIT_NOACTION as _,
    relevant_entry: ptr::null_mut(),
    first_entry: ptr::null_mut(),
};

/// Prepend an item to the front of the `__jit_debug_descriptor` entry list
///
/// # Safety
/// - Pointer to [`JITCodeEntry`] should point to a valid entry and stay alive
///   for the 'static lifetime
unsafe fn push_front(jce: *mut JITCodeEntry) {
    if __jit_debug_descriptor.first_entry.is_null() {
        __jit_debug_descriptor.first_entry = jce;
    } else {
        let old_first = __jit_debug_descriptor.first_entry;
        debug_assert!((*old_first).prev.is_null());
        (*jce).next = old_first;
        (*old_first).prev = jce;
        __jit_debug_descriptor.first_entry = jce;
    }
}

pub fn register_new_jit_code_entry(bytes: &'static [u8], action: JITAction) -> *mut JITCodeEntry {
    let entry: *mut JITCodeEntry = Box::into_raw(Box::new(JITCodeEntry {
        symfile_addr: bytes.as_ptr(),
        symfile_size: bytes.len() as _,
        ..JITCodeEntry::default()
    }));

    unsafe {
        push_front(entry);
        __jit_debug_descriptor.relevant_entry = entry;
        __jit_debug_descriptor.action_flag = action as u32;
    }

    entry
}
