use crate::unwind::UnwindFrame;
#[cfg(feature = "unwind")]
use crate::unwind::create_systemv_cie;
#[cfg(feature = "unwind")]
use gimli::{
    constants,
    write::{EhFrame, FrameTable},
};
use object::{
    SymbolFlags, SymbolKind, SymbolScope,
    write::{StandardSection, Symbol, SymbolSection},
};
use std::path::{Path, PathBuf};
#[cfg(feature = "unwind")]
use wasmer_compiler::dwarf::{DwarfState, WriterRelocate};
use wasmer_compiler::{
    elf::{add_relocations, emit_trap_section, save_object},
    misc::{CompiledFunctionExt, CompiledKind},
    object::get_object_for_target,
    types::function::CompiledFunction,
};
#[cfg(feature = "unwind")]
use wasmer_compiler::elf::emit_eh_frame_section;
use wasmer_types::{CompileError, LocalFunctionIndex, target::Target};

pub(crate) use wasmer_compiler::elf::{
    CompileOutput, emit_function_body, emit_import_trampoline, link_module,
};

pub(crate) fn emit_local_function(
    target: &Target,
    build_directory: &Path,
    index: LocalFunctionIndex,
    function: CompiledFunction,
    fde: Option<UnwindFrame>,
    #[cfg(feature = "unwind")] mut dwarf_state: Option<DwarfState>,
) -> Result<PathBuf, CompileError> {
    let kind = CompiledKind::Local(index, String::new());
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: kind.linkage_name().into_bytes(),
        value: 0,
        size: function.body.body.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text = object.section_id(StandardSection::Text);
    object.add_symbol_data(symbol, text, &function.body.body, 4);
    add_relocations(
        &mut object,
        text,
        &function.relocations,
        Some((index, symbol)),
    )?;

    #[cfg(feature = "unwind")]
    if let Some(dwarf_state) = dwarf_state.as_mut() {
        dwarf_state.write_sections(
            &mut object,
            symbol,
            function.body.body.len() as u64,
            target.triple().endianness().ok(),
        )?;
    }

    emit_trap_section(&mut object, &kind, &function.frame_info.traps);

    #[cfg(feature = "unwind")]
    if let Some(fde) = fde
        && let Some(mut cie) = create_systemv_cie(target.triple().architecture)
    {
        // The ELF image may be mapped at any base address, so make the FDE's
        // function reference position-independent.
        cie.fde_address_encoding = constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4;
        let mut frametable = FrameTable::default();
        let cie_id = frametable.add_cie(cie);
        match fde {
            UnwindFrame::SystemV(fde) => {
                frametable.add_fde(cie_id, fde);
            }
        }
        let mut eh_frame = EhFrame(WriterRelocate::new());
        frametable
            .write_eh_frame(&mut eh_frame)
            .map_err(|e| CompileError::Codegen(format!("failed to write .eh_frame: {e}")))?;

        let relocations = eh_frame.0.relocs.clone();
        emit_eh_frame_section(
            &mut object,
            &eh_frame.0.into_bytes(),
            &relocations,
            symbol,
            None,
        )?;
    }
    #[cfg(not(feature = "unwind"))]
    let _ = fde;

    save_object(object, build_directory, kind.object_filename())
}
