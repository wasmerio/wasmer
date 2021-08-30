// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use cranelift_codegen::MachSrcLoc;
use cranelift_codegen::{isa, Context};
use wasmer_compiler::{wasmparser::Range, FunctionAddressMap, InstructionAddressMap, SourceLoc};

pub fn get_function_address_map<'data>(
    context: &Context,
    range: Range,
    body_len: usize,
    isa: &dyn isa::TargetIsa,
) -> FunctionAddressMap {
    let mut instructions = Vec::new();

    if let Some(ref mcr) = &context.mach_compile_result {
        // New-style backend: we have a `MachCompileResult` that will give us `MachSrcLoc` mapping
        // tuples.
        for &MachSrcLoc { start, end, loc } in mcr.buffer.get_srclocs_sorted() {
            instructions.push(InstructionAddressMap {
                srcloc: SourceLoc::new(loc.bits()),
                code_offset: start as usize,
                code_len: (end - start) as usize,
            });
        }
    } else {
        let func = &context.func;
        let mut blocks = func.layout.blocks().collect::<Vec<_>>();
        blocks.sort_by_key(|block| func.offsets[*block]); // Ensure inst offsets always increase

        let encinfo = isa.encoding_info();
        for block in blocks {
            for (offset, inst, size) in func.inst_offsets(block, &encinfo) {
                let srcloc = func.srclocs[inst];
                instructions.push(InstructionAddressMap {
                    srcloc: SourceLoc::new(srcloc.bits()),
                    code_offset: offset as usize,
                    code_len: size as usize,
                });
            }
        }
    }

    // Generate artificial srcloc for function start/end to identify boundary
    // within module. Similar to FuncTranslator::cur_srcloc(): it will wrap around
    // if byte code is larger than 4 GB.
    let start_srcloc = SourceLoc::new(range.start as u32);
    let end_srcloc = SourceLoc::new(range.end as u32);

    FunctionAddressMap {
        instructions,
        start_srcloc,
        end_srcloc,
        body_offset: 0,
        body_len,
    }
}
