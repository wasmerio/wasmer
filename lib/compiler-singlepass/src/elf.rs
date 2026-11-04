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
    write::{Object, Relocation, StandardSection, Symbol, SymbolId, SymbolSection},
};

#[cfg(feature = "unwind")]
use wasmer_compiler::dwarf::{DwarfState, WriterRelocate};
#[cfg(feature = "unwind")]
use wasmer_compiler::elf::emit_eh_frame_section;
use wasmer_compiler::{
    elf::{add_undefined_symbol, emit_trap_section},
    get_object_for_target,
    misc::{CompiledFunctionExt, CompiledKind},
};
use wasmer_types::{CompileError, LocalFunctionIndex, TrapInformation, target::Target};

pub(crate) use wasmer_compiler::elf::{
    CompileOutput, emit_function_body, emit_import_trampoline, link_module,
};

pub(crate) struct FunctionObject {
    object: Object<'static>,
    symbol: SymbolId,
    kind: CompiledKind,
    pub relocations: Vec<Relocation>,
}

impl FunctionObject {
    pub fn new(target: &Target, index: LocalFunctionIndex) -> Result<Self, CompileError> {
        let kind = CompiledKind::Local(index, String::new());
        let mut object = get_object_for_target(target.triple())
            .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
        let symbol = object.add_symbol(Symbol {
            name: kind.linkage_name().into_bytes(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        Ok(Self {
            object,
            symbol,
            kind,
            relocations: Vec::new(),
        })
    }

    pub fn symbol_for_target(&mut self, kind: CompiledKind) -> SymbolId {
        let name = kind.linkage_name();
        if name == self.kind.linkage_name() {
            self.symbol
        } else {
            add_undefined_symbol(&mut self.object, name)
        }
    }

    pub fn emit(
        self,
        target: &Target,
        body: &[u8],
        traps: &[TrapInformation],
        fde: Option<UnwindFrame>,
        #[cfg(feature = "unwind")] mut dwarf_state: Option<DwarfState>,
    ) -> Result<Vec<u8>, CompileError> {
        let Self {
            mut object,
            symbol,
            kind,
            relocations,
        } = self;
        let text = object.section_id(StandardSection::Text);
        object.add_symbol_data(symbol, text, body, 4);
        for relocation in relocations {
            object
                .add_relocation(text, relocation)
                .map_err(|e| CompileError::Codegen(format!("failed to add ELF relocation: {e}")))?;
        }

        // TODO: support dSYM debug info file creation and loading in the future
        #[cfg(feature = "unwind")]
        if matches!(
            target.triple().binary_format,
            target_lexicon::BinaryFormat::Elf
        ) && let Some(dwarf_state) = dwarf_state.as_mut()
        {
            dwarf_state.write_sections(
                &mut object,
                symbol,
                body.len() as u64,
                target.triple().endianness().ok(),
            )?;
        }

        emit_trap_section(&mut object, &kind, traps);

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

        object
            .write()
            .map_err(|e| CompileError::Codegen(format!("failed to serialize object: {e}")))
    }
}
