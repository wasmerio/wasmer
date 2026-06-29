/*
 * ! Remove me once rkyv generates doc-comments for fields or generates an #[allow(missing_docs)]
 * on their own.
 */
#![allow(missing_docs)]

//! Relocation is the process of assigning load addresses for position-dependent
//! code and data of a program and adjusting the code and data to reflect the
//! assigned addresses.
//!
//! [Learn more](https://en.wikipedia.org/wiki/Relocation_(computing)).
//!
//! Each time a `Compiler` compiles a WebAssembly function (into machine code),
//! it also attaches if there are any relocations that need to be patched into
//! the generated machine code, so a given frontend (JIT or native) can
//! do the corresponding work to run it.

use super::section::SectionIndex;
use crate::{Addend, CodeOffset};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::{FunctionIndex, LibCall, LocalFunctionIndex, entity::PrimaryMap};

/// Relocation kinds for every ISA.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Copy, Clone, Debug, PartialEq, Eq)]
#[rkyv(derive(Debug), compare(PartialEq))]
#[repr(u8)]
pub enum RelocationKind {
    /// absolute 4-byte
    Abs4,
    /// absolute 8-byte
    Abs8,

    /// PC-relative 4-byte
    PCRel4,
    /// PC-relative 8-byte
    PCRel8,

    /// x86 call to PC-relative 4-byte
    X86CallPCRel4,
    /// x86 call to PLT-relative 4-byte
    X86CallPLTRel4,
    /// x86 GOT PC-relative 4-byte
    X86GOTPCRel4,

    /// R_AARCH64_ADR_PREL_LO21
    Aarch64AdrPrelLo21,

    /// R_AARCH64_ADR_PREL_PG_HI21
    Aarch64AdrPrelPgHi21,

    /// R_AARCH64_ADD_ABS_LO12_NC
    Aarch64AddAbsLo12Nc,

    /// R_AARCH64_LDST128_ABS_LO12_NC
    Aarch64Ldst128AbsLo12Nc,

    /// R_AARCH64_LDST64_ABS_LO12_NC
    Aarch64Ldst64AbsLo12Nc,

    /// Arm32 call target
    Arm32Call,
    /// Arm64 call target
    Arm64Call,
    /// Arm64 movk/z part 0
    Arm64Movw0,
    /// Arm64 movk/z part 1
    Arm64Movw1,
    /// Arm64 movk/z part 2
    Arm64Movw2,
    /// Arm64 movk/z part 3
    Arm64Movw3,
    /// RISC-V PC-relative high 20bit
    RiscvPCRelHi20,
    /// RISC-V PC-relative low 12bit, I-type
    RiscvPCRelLo12I,
    /// RISC-V call target
    RiscvCall,
    /// LoongArch absolute high 20bit
    LArchAbsHi20,
    /// LoongArch absolute low 12bit
    LArchAbsLo12,
    /// LoongArch absolute high 12bit
    LArchAbs64Hi12,
    /// LoongArch absolute low 20bit
    LArchAbs64Lo20,
    /// LoongArch PC-relative call 38bit
    LArchCall36,
    /// LoongArch PC-relative high 20bit
    LArchPCAlaHi20,
    /// LoongArch PC-relative low 12bit
    LArchPCAlaLo12,
    /// LoongArch PC64-relative high 12bit
    LArchPCAla64Hi12,
    /// LoongArch PC64-relative low 20bit
    LArchPCAla64Lo20,
    /// Elf x86_64 32 bit signed PC relative offset to two GOT entries for GD symbol.
    ElfX86_64TlsGd,
    // /// Mach-O x86_64 32 bit signed PC relative offset to a `__thread_vars` entry.
    // MachOX86_64Tlv,

    // -- Mach-O-specific relocations
    //
    // --- Arm64
    // (MACHO_ARM64_RELOC_UNSIGNED) for pointers
    MachoArm64RelocUnsigned,
    // (MACHO_ARM64_RELOC_SUBTRACTOR) must be followed by a ARM64_RELOC_UNSIGNED
    MachoArm64RelocSubtractor,
    // (MACHO_ARM64_RELOC_BRANCH26) a B/BL instruction with 26-bit displacement
    MachoArm64RelocBranch26,
    // (MACHO_ARM64_RELOC_PAGE21) pc-rel distance to page of target
    MachoArm64RelocPage21,
    // (MACHO_ARM64_RELOC_PAGEOFF12) offset within page, scaled by r_length
    MachoArm64RelocPageoff12,
    // (MACHO_ARM64_RELOC_GOT_LOAD_PAGE21) pc-rel distance to page of GOT slot
    MachoArm64RelocGotLoadPage21,
    // (MACHO_ARM64_RELOC_GOT_LOAD_PAGEOFF12) offset within page of GOT slot, scaled by r_length
    MachoArm64RelocGotLoadPageoff12,
    // (MACHO_ARM64_RELOC_POINTER_TO_GOT) for pointers to GOT slots
    MachoArm64RelocPointerToGot,
    // (MACHO_ARM64_RELOC_TLVP_LOAD_PAGE21) pc-rel distance to page of TLVP slot
    MachoArm64RelocTlvpLoadPage21,
    // (MACHO_ARM64_RELOC_TLVP_LOAD_PAGEOFF12) offset within page of TLVP slot, scaled by r_length
    MachoArm64RelocTlvpLoadPageoff12,
    // (MACHO_ARM64_RELOC_ADDEND) must be followed by PAGE21 or PAGEOFF12
    MachoArm64RelocAddend,

    // --- X86_64
    // (MACHO_X86_64_RELOC_UNSIGNED) for absolute addresses
    MachoX86_64RelocUnsigned,
    // (MACHO_X86_64_RELOC_SIGNED) for signed 32-bit displacement
    MachoX86_64RelocSigned,
    // (MACHO_X86_64_RELOC_BRANCH) a CALL/JMP instruction with 32-bit displacement
    MachoX86_64RelocBranch,
    // (MACHO_X86_64_RELOC_GOT_LOAD) a MOVQ load of a GOT entry
    MachoX86_64RelocGotLoad,
    // (MACHO_X86_64_RELOC_GOT) other GOT references
    MachoX86_64RelocGot,
    // (MACHO_X86_64_RELOC_SUBTRACTOR) must be followed by a X86_64_RELOC_UNSIGNED
    MachoX86_64RelocSubtractor,
    // (MACHO_X86_64_RELOC_SIGNED_1) for signed 32-bit displacement with a -1 addend
    MachoX86_64RelocSigned1,
    // (MACHO_X86_64_RELOC_SIGNED_2) for signed 32-bit displacement with a -2 addend
    MachoX86_64RelocSigned2,
    // (MACHO_X86_64_RELOC_SIGNED_4) for signed 32-bit displacement with a -4 addend
    MachoX86_64RelocSigned4,
    // (MACHO_X86_64_RELOC_TLV) for thread local variables
    MachoX86_64RelocTlv,

    // TODO: sort the items when we bump the rkyv version
    /// absolute 6 bits
    Abs6Bits,
    /// absolute 1-byte
    Abs,
    /// absolute 2-byte
    Abs2,

    /// addition at the place of the relocation (1-byte)
    Add,
    /// addition at the place of the relocation (2-bytes)
    Add2,
    /// addition at the place of the relocation (4-bytes)
    Add4,
    /// addition at the place of the relocation (8-bytes)
    Add8,

    /// subtraction at the place of the relocation (6 bits)
    Sub6Bits,
    /// subtraction at the place of the relocation (1-byte)
    Sub,
    /// subtraction at the place of the relocation (2-bytes)
    Sub2,
    /// subtraction at the place of the relocation (4-bytes)
    Sub4,
    /// subtraction at the place of the relocation (8-bytes)
    Sub8,
}

/// A record of a relocation to perform.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, Clone, PartialEq, Eq)]
#[rkyv(derive(Debug), compare(PartialEq))]
pub struct Relocation {
    /// The relocation kind.
    pub kind: RelocationKind,
    /// Relocation target.
    pub reloc_target: RelocationTarget,
    /// The offset where to apply the relocation.
    pub offset: CodeOffset,
    /// The addend to add to the relocation value.
    pub addend: Addend,
}

/// Destination function. Can be either user function or some special one, like `memory.grow`.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[rkyv(derive(Debug, Hash, PartialEq, Eq), compare(PartialEq))]
#[repr(u8)]
pub enum RelocationTarget {
    /// A relocation to a function defined locally in the wasm (not an imported one).
    LocalFunc(LocalFunctionIndex),
    /// A relocation to a dynamic trampoline.
    DynamicTrampoline(FunctionIndex),
    /// A compiler-generated libcall.
    LibCall(LibCall),
    /// Custom sections generated by the compiler
    CustomSection(SectionIndex),
}

/// Relocations to apply to function bodies.
pub type Relocations = PrimaryMap<LocalFunctionIndex, Vec<Relocation>>;
