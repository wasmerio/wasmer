//! Linking for Universal-compiled code.

use crate::{
    get_libcall_trampoline,
    types::{
        relocation::{RelocationKind, RelocationLike, RelocationTarget},
        section::SectionIndex,
    },
    FunctionExtent,
};
use std::{
    collections::{HashMap, HashSet},
    ptr::{read_unaligned, write_unaligned},
};

use wasmer_types::{entity::PrimaryMap, LocalFunctionIndex, ModuleInfo};
use wasmer_vm::{libcalls::function_pointer, SectionBodyPtr};

#[allow(clippy::too_many_arguments)]
fn apply_relocation(
    body: usize,
    r: &impl RelocationLike,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    libcall_trampolines_sec_idx: SectionIndex,
    libcall_trampoline_len: usize,
    riscv_pcrel_hi20s: &mut HashMap<usize, u32>,
    get_got_address: &dyn Fn(RelocationTarget) -> Option<usize>,
) {
    let reloc_target = r.reloc_target();

    // Note: if the relocation needs GOT and its addend is not zero we will relax the
    // relocation and, instead of making it use the GOT entry, we will fixup the assembly to
    // use the final pointer directly, without any indirection. Also, see the comment in
    // compiler-llvm/src/object_file.rs:288.
    let target_func_address: usize = if r.kind().needs_got() && r.addend() == 0 {
        if let Some(got_address) = get_got_address(reloc_target) {
            got_address
        } else {
            panic!("No GOT entry for reloc target {reloc_target:?}")
        }
    } else {
        match reloc_target {
            RelocationTarget::LocalFunc(index) => *allocated_functions[index].ptr as usize,
            RelocationTarget::LibCall(libcall) => {
                // Use the direct target of the libcall if the relocation supports
                // a full 64-bit address. Otherwise use a trampoline.
                if matches!(
                    r.kind(),
                    RelocationKind::Abs8
                        | RelocationKind::X86PCRel8
                        | RelocationKind::MachoArm64RelocUnsigned
                        | RelocationKind::MachoX86_64RelocUnsigned
                ) {
                    function_pointer(libcall)
                } else {
                    get_libcall_trampoline(
                        libcall,
                        allocated_sections[libcall_trampolines_sec_idx].0 as usize,
                        libcall_trampoline_len,
                    )
                }
            }
            RelocationTarget::CustomSection(custom_section) => {
                *allocated_sections[custom_section] as usize
            }
        }
    };

    // A set of addresses at which a SUBTRACTOR relocation was applied.
    let mut macho_aarch64_subtractor_addresses = HashSet::new();

    match r.kind() {
        RelocationKind::Abs8 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },
        RelocationKind::X86PCRel4 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u32, reloc_delta as _);
        },
        RelocationKind::X86PCRel8 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },
        RelocationKind::X86CallPCRel4 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u32, reloc_delta as _);
        },
        RelocationKind::Arm64Call => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            if (reloc_delta as i64).abs() >= 0x1000_0000 {
                panic!(
                    "Relocation to big for {:?} for {:?} with {:x}, current val {:x}",
                    r.kind(),
                    r.reloc_target(),
                    reloc_delta,
                    read_unaligned(reloc_address as *mut u32)
                )
            }
            let reloc_delta = (((reloc_delta / 4) as u32) & 0x3ff_ffff)
                | (read_unaligned(reloc_address as *mut u32) & 0xfc00_0000);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw0 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta =
                (((reloc_delta & 0xffff) as u32) << 5) | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw1 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 16) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw2 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 32) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw3 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 48) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::RiscvPCRelHi20 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);

            // save for later reference with RiscvPCRelLo12I
            riscv_pcrel_hi20s.insert(reloc_address, reloc_delta as u32);

            let reloc_delta = ((reloc_delta.wrapping_add(0x800) & 0xfffff000) as u32)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::RiscvPCRelLo12I => unsafe {
            let (reloc_address, reloc_abs) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((riscv_pcrel_hi20s.get(&(reloc_abs as usize)).expect(
                "R_RISCV_PCREL_LO12_I relocation target must be a symbol with R_RISCV_PCREL_HI20",
            ) & 0xfff)
                << 20)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::RiscvCall => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((reloc_delta & 0xfff) << 52)
                | (reloc_delta.wrapping_add(0x800) & 0xfffff000)
                | read_unaligned(reloc_address as *mut u64);
            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },
        RelocationKind::LArchAbsHi20 | RelocationKind::LArchPCAlaHi20 => unsafe {
            let (reloc_address, reloc_abs) = r.for_address(body, target_func_address as u64);
            let reloc_abs = ((((reloc_abs >> 12) & 0xfffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_abs);
        },
        RelocationKind::LArchAbsLo12 | RelocationKind::LArchPCAlaLo12 => unsafe {
            let (reloc_address, reloc_abs) = r.for_address(body, target_func_address as u64);
            let reloc_abs =
                (((reloc_abs & 0xfff) as u32) << 10) | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_abs);
        },
        RelocationKind::LArchAbs64Hi12 | RelocationKind::LArchPCAla64Hi12 => unsafe {
            let (reloc_address, reloc_abs) = r.for_address(body, target_func_address as u64);
            let reloc_abs = ((((reloc_abs >> 52) & 0xfff) as u32) << 10)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_abs);
        },
        RelocationKind::LArchAbs64Lo20 | RelocationKind::LArchPCAla64Lo20 => unsafe {
            let (reloc_address, reloc_abs) = r.for_address(body, target_func_address as u64);
            let reloc_abs = ((((reloc_abs >> 32) & 0xfffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_abs);
        },
        RelocationKind::LArchCall36 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta1 = ((((reloc_delta >> 18) & 0xfffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta1);
            let reloc_delta2 = ((((reloc_delta >> 2) & 0xffff) as u32) << 10)
                | read_unaligned((reloc_address + 4) as *mut u32);
            write_unaligned((reloc_address + 4) as *mut u32, reloc_delta2);
        },
        RelocationKind::Aarch64AdrPrelPgHi21 => unsafe {
            let (reloc_address, delta) = r.for_address(body, target_func_address as u64);

            let delta = delta as isize;
            assert!(
                ((-1 << 32)..(1 << 32)).contains(&delta),
                "can't generate page-relative relocation with ±4GB `adrp` instruction"
            );

            let op = read_unaligned(reloc_address as *mut u32);
            let delta = delta >> 12;
            let immlo = ((delta as u32) & 0b11) << 29;
            let immhi = (((delta as u32) >> 2) & 0x7ffff) << 5;
            let mask = !((0x7ffff << 5) | (0b11 << 29));
            let op = (op & mask) | immlo | immhi;

            write_unaligned(reloc_address as *mut u32, op);
        },
        RelocationKind::Aarch64AdrPrelLo21 => unsafe {
            let (reloc_address, delta) = r.for_address(body, target_func_address as u64);

            let delta = delta as isize;
            assert!(
                ((-1 << 20)..(1 << 20)).contains(&delta),
                "can't generate an ADR_PREL_LO21 relocation with an immediate larger than 20 bits"
            );

            let op = read_unaligned(reloc_address as *mut u32);
            let immlo = ((delta as u32) & 0b11) << 29;
            let immhi = (((delta as u32) >> 2) & 0x7ffff) << 5;
            let mask = !((0x7ffff << 5) | (0b11 << 29));
            let op = (op & mask) | immlo | immhi;

            write_unaligned(reloc_address as *mut u32, op);
        },
        RelocationKind::Aarch64AddAbsLo12Nc => unsafe {
            let (reloc_address, delta) = r.for_address(body, target_func_address as u64);

            let delta = delta as isize;
            let op = read_unaligned(reloc_address as *mut u32);
            let imm = ((delta as u32) & 0xfff) << 10;
            let mask = !((0xfff) << 10);
            let op = (op & mask) | imm;

            write_unaligned(reloc_address as *mut u32, op);
        },
        RelocationKind::Aarch64Ldst128AbsLo12Nc => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((reloc_delta as u32 & 0xfff) >> 4) << 10
                | (read_unaligned(reloc_address as *mut u32) & 0xFFC003FF);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Aarch64Ldst64AbsLo12Nc => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((reloc_delta as u32 & 0xfff) >> 3) << 10
                | (read_unaligned(reloc_address as *mut u32) & 0xFFC003FF);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::MachoArm64RelocSubtractor | RelocationKind::MachoX86_64RelocSubtractor => unsafe {
            let (reloc_address, reloc_sub) = r.for_address(body, target_func_address as u64);
            macho_aarch64_subtractor_addresses.insert(reloc_address);
            write_unaligned(reloc_address as *mut u64, reloc_sub);
        },
        RelocationKind::MachoArm64RelocGotLoadPage21
        | RelocationKind::MachoArm64RelocTlvpLoadPage21 => unsafe {
            let (reloc_address, _) = r.for_address(body, target_func_address as u64);
            let target_func_page = target_func_address & !0xfff;
            let reloc_at_page = reloc_address & !0xfff;
            let pcrel = (target_func_page as isize)
                .checked_sub(reloc_at_page as isize)
                .unwrap();
            assert!(
                (-1 << 32) <= (pcrel as i64) && (pcrel as i64) < (1 << 32),
                "can't reach GOT page with ±4GB `adrp` instruction"
            );
            let val = pcrel >> 12;

            let immlo = ((val as u32) & 0b11) << 29;
            let immhi = (((val as u32) >> 2) & 0x7ffff) << 5;
            let mask = !((0x7ffff << 5) | (0b11 << 29));
            let op = read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, (op & mask) | immlo | immhi);
        },

        RelocationKind::MachoArm64RelocPage21 => unsafe {
            let target_page: u64 =
                ((target_func_address.wrapping_add(r.addend() as _)) & !0xfff) as u64;
            let reloc_address = body.wrapping_add(r.offset() as _);
            let pc_page: u64 = (reloc_address & !0xfff) as u64;
            let page_delta = target_page - pc_page;
            let raw_instr = read_unaligned(reloc_address as *mut u32);
            assert_eq!(
                (raw_instr & 0xffffffe0),
                0x90000000,
                "raw_instr isn't an ADRP instruction"
            );

            let immlo: u32 = ((page_delta >> 12) & 0x3) as _;
            let immhi: u32 = ((page_delta >> 14) & 0x7ffff) as _;
            let fixed_instr = raw_instr | (immlo << 29) | (immhi << 5);
            write_unaligned(reloc_address as *mut u32, fixed_instr);
        },
        RelocationKind::MachoArm64RelocPageoff12 => unsafe {
            let target_offset: u64 =
                ((target_func_address.wrapping_add(r.addend() as _)) & 0xfff) as u64;

            let reloc_address = body.wrapping_add(r.offset() as _);
            let raw_instr = read_unaligned(reloc_address as *mut u32);
            let imm_shift = {
                const VEC128_MASK: u32 = 0x04800000;

                const LOAD_STORE_IMM12_MASK: u32 = 0x3b000000;
                let is_load_store_imm12 = (raw_instr & LOAD_STORE_IMM12_MASK) == 0x39000000;

                if is_load_store_imm12 {
                    let mut implicit_shift = raw_instr >> 30;

                    if implicit_shift == 0 && (raw_instr & VEC128_MASK) == VEC128_MASK {
                        implicit_shift = 4;
                    }

                    implicit_shift
                } else {
                    0
                }
            };

            assert_eq!(
                target_offset & ((1 << imm_shift) - 1),
                0,
                "PAGEOFF12 target is not aligned"
            );

            let encoded_imm: u32 = ((target_offset as u32) >> imm_shift) << 10;
            let fixed_instr: u32 = raw_instr | encoded_imm;
            write_unaligned(reloc_address as *mut u32, fixed_instr);
        },

        RelocationKind::MachoArm64RelocGotLoadPageoff12 => unsafe {
            // See comment at the top of the function. TLDR: if addend != 0 we can't really use the
            // GOT entry. We fixup this relocation to use a `add` rather than a `ldr` instruction,
            // skipping the indirection from the GOT.
            if r.addend() == 0 {
                let (reloc_address, _) = r.for_address(body, target_func_address as u64);
                assert_eq!(target_func_address & 0b111, 0);
                let val = target_func_address >> 3;
                let imm9 = ((val & 0x1ff) << 10) as u32;
                let mask = !(0x1ff << 10);
                let op = read_unaligned(reloc_address as *mut u32);
                write_unaligned(reloc_address as *mut u32, (op & mask) | imm9);
            } else {
                let fixup_ptr = body + r.offset() as usize;
                let target_address: usize = target_func_address + r.addend() as usize;

                let raw_instr = read_unaligned(fixup_ptr as *mut u32);

                assert_eq!(
                    raw_instr & 0xfffffc00, 0xf9400000,
                    "raw_instr isn't a 64-bit LDR immediate (bits: {raw_instr:032b}, hex: {raw_instr:x})"
                );

                let reg: u32 = raw_instr & 0b11111;

                let mut fixup_ldr = 0x91000000 | (reg << 5) | reg;
                fixup_ldr |= ((target_address & 0xfff) as u32) << 10;

                write_unaligned(fixup_ptr as *mut u32, fixup_ldr);
            }
        },
        RelocationKind::MachoArm64RelocUnsigned | RelocationKind::MachoX86_64RelocUnsigned => unsafe {
            let (reloc_address, mut reloc_delta) = r.for_address(body, target_func_address as u64);

            if macho_aarch64_subtractor_addresses.contains(&reloc_address) {
                reloc_delta -= read_unaligned(reloc_address as *mut u64);
            }

            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },

        RelocationKind::MachoArm64RelocPointerToGot => unsafe {
            let at = body + r.offset() as usize;
            let pcrel = i32::try_from((target_func_address as isize) - (at as isize)).unwrap();
            write_unaligned(at as *mut i32, pcrel);
        },

        RelocationKind::MachoArm64RelocBranch26 => unsafe {
            let fixup_ptr = body + r.offset() as usize;
            assert_eq!(fixup_ptr & 0x3, 0, "Branch-inst is not 32-bit aligned");
            let value = i32::try_from((target_func_address as isize) - (fixup_ptr as isize))
                .unwrap()
                .wrapping_add(r.addend() as _);
            assert!(
                value & 0x3 == 0,
                "BranchPCRel26 target is not 32-bit aligned"
            );

            assert!(
                (-(1 << 27)..=((1 << 27) - 1)).contains(&value),
                "out of range BranchPCRel26 target"
            );

            let raw_instr = read_unaligned(fixup_ptr as *mut u32);

            assert_eq!(
                raw_instr & 0x7fffffff,
                0x14000000,
                "RawInstr isn't a B or BR immediate instruction"
            );
            let imm: u32 = ((value as u32) & ((1 << 28) - 1)) >> 2;
            let fixed_instr: u32 = raw_instr | imm;

            write_unaligned(fixup_ptr as *mut u32, fixed_instr);
        },
        kind => panic!("Relocation kind unsupported in the current architecture: {kind}"),
    }
}

/// Links a module, patching the allocated functions with the
/// required relocations and jump tables.
#[allow(clippy::too_many_arguments)]
pub fn link_module<'a>(
    _module: &ModuleInfo,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
    function_relocations: impl Iterator<
        Item = (
            LocalFunctionIndex,
            impl Iterator<Item = &'a (impl RelocationLike + 'a)>,
        ),
    >,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    section_relocations: impl Iterator<
        Item = (
            SectionIndex,
            impl Iterator<Item = &'a (impl RelocationLike + 'a)>,
        ),
    >,
    libcall_trampolines: SectionIndex,
    trampoline_len: usize,
    get_got_address: &'a dyn Fn(RelocationTarget) -> Option<usize>,
) {
    let mut riscv_pcrel_hi20s: HashMap<usize, u32> = HashMap::new();

    for (i, section_relocs) in section_relocations {
        let body = *allocated_sections[i] as usize;
        for r in section_relocs {
            apply_relocation(
                body,
                r,
                allocated_functions,
                allocated_sections,
                libcall_trampolines,
                trampoline_len,
                &mut riscv_pcrel_hi20s,
                get_got_address,
            );
        }
    }
    for (i, function_relocs) in function_relocations {
        let body = *allocated_functions[i].ptr as usize;
        for r in function_relocs {
            apply_relocation(
                body,
                r,
                allocated_functions,
                allocated_sections,
                libcall_trampolines,
                trampoline_len,
                &mut riscv_pcrel_hi20s,
                get_got_address,
            );
        }
    }
}
