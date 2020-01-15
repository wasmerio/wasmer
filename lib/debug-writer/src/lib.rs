use std::str::FromStr;

use target_lexicon::{Triple, Architecture, Vendor, OperatingSystem, Environment, BinaryFormat};
use gimli::write::{DwarfUnit, Sections, Address, RangeList, EndianVec, AttributeValue, Range};

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
const HOST: X86_64_WINDOWS = Triple {
    architecture: Architecture::X86_64,
    vendor: Vendor::Pc,
    operating_system: OperatingSystem::Windows,
    environment: Environment::Msvc,
    binary_format: BinaryFormat::Coff,
};


pub fn generate_dwarf(module_info: &ModuleInfo, code_version: CodeVersion, platform: Triple) -> Result<Vec<u8>, String> {
    // copied from https://docs.rs/gimli/0.20.0/gimli/write/index.html ; TODO: review these values
    let encoding = gimli::Encoding {
        format: gimli::Format::Dwarf64,
        version: 5,
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
    // Create a `Vec` for each DWARF section.
    let mut sections = Sections::new(EndianVec::new(gimli::LittleEndian));
    // Finally, write the DWARF data to the sections.
    dwarf.write(&mut sections).map_err(|e| e.to_string())?;
    sections.for_each(|id, data| {
        // Here you can add the data to the output object file.
        Ok(())
    });
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
    // - `DW_AT_ranges` is for non-contiguous ranges of address but says that it's useful for
    //    subrountine` 
    
    match platform {
        X86_64_GNU_LINUX => unimplemented!("in progress"),
        X86_64_OSX => unimplemented!("in progress"),
        X86_64_WINDOWS => unimplemented!("in progress"),
        _ => return Err(format!("Debug output for the platform {} is not yet supported", platform)),
    }
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
