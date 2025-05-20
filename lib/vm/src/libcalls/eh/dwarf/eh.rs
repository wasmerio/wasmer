//! Parsing of GCC-style Language-Specific Data Area (LSDA)
//! For details se*const ():
//!  * <https://refspecs.linuxfoundation.org/LSB_3.0.0/LSB-PDA/LSB-PDA/ehframechpt.html>
//!  * <https://refspecs.linuxfoundation.org/LSB_5.0.0/LSB-Core-generic/LSB-Core-generic/dwarfext.html>
//!  * <https://itanium-cxx-abi.github.io/cxx-abi/exceptions.pdf>
//!  * <https://www.airs.com/blog/archives/460>
//!  * <https://www.airs.com/blog/archives/464>
//!
//! A reference implementation may be found in the GCC source tree
//! (`<root>/libgcc/unwind-c.c` as of this writing).

#![allow(non_upper_case_globals)]
#![allow(clippy::transmutes_expressible_as_ptr_casts)]
#![allow(clippy::comparison_chain)]
#![allow(unused)]

use core::{mem, ptr};

use super::DwarfReader;

pub const DW_EH_PE_omit: u8 = 0xFF;
pub const DW_EH_PE_absptr: u8 = 0x00;

pub const DW_EH_PE_uleb128: u8 = 0x01;
pub const DW_EH_PE_udata2: u8 = 0x02;
pub const DW_EH_PE_udata4: u8 = 0x03;
pub const DW_EH_PE_udata8: u8 = 0x04;
pub const DW_EH_PE_sleb128: u8 = 0x09;
pub const DW_EH_PE_sdata2: u8 = 0x0A;
pub const DW_EH_PE_sdata4: u8 = 0x0B;
pub const DW_EH_PE_sdata8: u8 = 0x0C;

pub const DW_EH_PE_pcrel: u8 = 0x10;
pub const DW_EH_PE_textrel: u8 = 0x20;
pub const DW_EH_PE_datarel: u8 = 0x30;
pub const DW_EH_PE_funcrel: u8 = 0x40;
pub const DW_EH_PE_aligned: u8 = 0x50;

pub const DW_EH_PE_indirect: u8 = 0x80;

#[derive(Copy, Clone)]
pub struct EHContext<'a> {
    pub ip: *const u8,                             // Current instruction pointer
    pub func_start: *const u8,                     // Pointer to the current function
    pub get_text_start: &'a dyn Fn() -> *const u8, // Get pointer to the code section
    pub get_data_start: &'a dyn Fn() -> *const u8, // Get pointer to the data section
    pub tag: u64,                                  // The tag associated with the WasmerException
}

/// Landing pad.
type LPad = *const u8;

#[derive(Debug, Clone)]
pub enum EHAction {
    None,
    Cleanup(LPad),
    Catch { lpad: LPad, tag: u64 },
    Filter { lpad: LPad, tag: u64 },
    Terminate,
}

/// 32-bit ARM Darwin platforms uses SjLj exceptions.
///
/// The exception is watchOS armv7k (specifically that subarchitecture), which
/// instead uses DWARF Call Frame Information (CFI) unwinding.
///
/// <https://github.com/llvm/llvm-project/blob/llvmorg-18.1.4/clang/lib/Driver/ToolChains/Darwin.cpp#L3107-L3119>
pub const USING_SJLJ_EXCEPTIONS: bool = cfg!(all(
    target_vendor = "apple",
    not(target_os = "watchos"),
    target_arch = "arm"
));

/* change to true to enable logging from the personality function */
macro_rules! log {
    ($e: expr) => {
        if false {
            eprintln!($e)
        }

    };

    ($($e: expr),*) => {
        if false {
            eprintln!($($e),*)
        }

    };
}

pub unsafe fn find_eh_action(lsda: *const u8, context: &EHContext<'_>) -> Result<EHAction, ()> {
    if lsda.is_null() {
        return Ok(EHAction::None);
    }

    log!("(pers) Analysing LSDA at {lsda:?}");

    let func_start = context.func_start;
    let mut reader = DwarfReader::new(lsda);

    let lpad_base = unsafe {
        let lp_start_encoding = reader.read::<u8>();

        log!("(pers) Read LP start encoding {lp_start_encoding:?}");
        // base address for landing pad offsets
        if lp_start_encoding != DW_EH_PE_omit {
            read_encoded_pointer(&mut reader, context, lp_start_encoding)?
        } else {
            log!("(pers) (is omit)");
            func_start
        }
    };
    log!("(pers) read landingpad base: {lpad_base:?}");

    let ttype_encoding = unsafe { reader.read::<u8>() };
    log!("(pers) read ttype encoding: {ttype_encoding:?}");

    // If no value for type_table_encoding was given it means that there's no
    // type_table, therefore we can't possibly use this lpad.
    if ttype_encoding == DW_EH_PE_omit {
        log!("(pers) ttype is omit, returning None");
        return Ok(EHAction::None);
    }

    let class_info = unsafe {
        let offset = reader.read_uleb128();
        log!("(pers) read class_info offset {offset:?}");
        reader.ptr.wrapping_add(offset as _)
    };
    log!("(pers) read class_info sits at offset {class_info:?}");

    let call_site_encoding = unsafe { reader.read::<u8>() };
    log!("(pers) read call_site_encoding is {call_site_encoding:?}");

    let action_table = unsafe {
        let call_site_table_length = reader.read_uleb128();
        log!("(pers) read call_site has length {call_site_table_length:?}");
        reader.ptr.wrapping_add(call_site_table_length as usize)
    };

    log!("(pers) action table sits at offset {action_table:?}");
    let ip = context.ip;

    if !USING_SJLJ_EXCEPTIONS {
        // read the callsite table
        while reader.ptr < action_table {
            unsafe {
                // these are offsets rather than pointers;
                let cs_start = read_encoded_offset(&mut reader, call_site_encoding)?;
                let cs_len = read_encoded_offset(&mut reader, call_site_encoding)?;
                let cs_lpad = read_encoded_offset(&mut reader, call_site_encoding)?;
                let cs_action_entry = reader.read_uleb128();

                log!("(pers) read cs_start is {cs_start:?}");
                log!("(pers) read cs_len is {cs_len:?}");
                log!("(pers) read cs_lpad is {cs_lpad:?}");
                log!("(pers) read cs_ae is {cs_action_entry:?}");
                // Callsite table is sorted by cs_start, so if we've passed the ip, we
                // may stop searching.
                if ip < func_start.wrapping_add(cs_start) {
                    break;
                }

                if ip < func_start.wrapping_add(cs_start + cs_len) {
                    log!(
                        "(pers) found a matching call site: {func_start:?} <= {ip:?} <= {:?}",
                        func_start.wrapping_add(cs_start + cs_len)
                    );
                    if cs_lpad == 0 {
                        return Ok(EHAction::None);
                    } else {
                        let lpad = lpad_base.wrapping_add(cs_lpad);

                        log!("(pers) lpad sits at {lpad:?}");

                        if cs_action_entry == 0 {
                            return Ok(EHAction::Cleanup(lpad));
                        }

                        log!("(pers) read cs_action_entry: {cs_action_entry}");
                        log!("(pers) action_table: {action_table:?}");

                        // Convert 1-based byte offset into
                        let mut action: *const u8 =
                            action_table.wrapping_add((cs_action_entry - 1) as usize);

                        log!("(pers) first action at: {action:?}");

                        loop {
                            let mut reader = DwarfReader::new(action);
                            let ttype_index = reader.read_sleb128();
                            log!(
                                "(pers) ttype_index for action #{cs_action_entry}: {ttype_index:?}"
                            );

                            if ttype_index > 0 {
                                if class_info.is_null() {
                                    panic!();
                                }

                                let tag_ptr = {
                                    let new_ttype_index = match ttype_encoding & 0x0f {
                                        DW_EH_PE_absptr => {
                                            ttype_index * (size_of::<*const u8>() as i64)
                                        }
                                        DW_EH_PE_sdata2 | DW_EH_PE_udata2 => ttype_index * 2,
                                        DW_EH_PE_sdata4 | DW_EH_PE_udata4 => ttype_index * 4,
                                        DW_EH_PE_sdata8 | DW_EH_PE_udata8 => ttype_index * 8,
                                        _ => panic!(),
                                    };

                                    log!("(pers) new_ttype_index for action #{cs_action_entry}: {new_ttype_index:?}");

                                    let i = class_info.wrapping_sub(new_ttype_index as usize);
                                    log!("(pers) reading ttype info from {i:?}");
                                    read_encoded_pointer(
                                        &mut DwarfReader::new(i),
                                        context,
                                        ttype_encoding,
                                    )
                                };
                                let tag_ptr = tag_ptr.unwrap();

                                if tag_ptr.is_null() {
                                    return Ok(EHAction::Catch { lpad, tag: 0 });
                                }

                                let tag = std::mem::transmute::<*const u8, *const u64>(tag_ptr)
                                    .read_unaligned();

                                if context.tag == tag {
                                    return Ok(EHAction::Catch { lpad, tag });
                                }
                            } else if ttype_index == 0 {
                                return Ok(EHAction::Cleanup(lpad));
                            }

                            let action_offset = reader.clone().read_sleb128();
                            if action_offset == 0 {
                                return Ok(EHAction::None);
                            }

                            action = reader.ptr.wrapping_add(action_offset as usize);
                        }
                    }
                }
            }
        }

        // Ip is not present in the table. This indicates a nounwind call.
        Ok(EHAction::Terminate)
    } else {
        todo!()
    }
}

#[inline]
fn get_encoding_size(encoding: u8) -> usize {
    if encoding == DW_EH_PE_omit {
        return 0;
    }

    match encoding & 0x0f {
        DW_EH_PE_absptr => size_of::<usize>(),
        DW_EH_PE_udata2 | DW_EH_PE_sdata2 => size_of::<u16>(),
        DW_EH_PE_udata4 | DW_EH_PE_sdata4 => size_of::<u32>(),
        DW_EH_PE_udata8 | DW_EH_PE_sdata8 => size_of::<u64>(),
        _ => panic!(),
    }
}

#[inline]
fn round_up(unrounded: usize, align: usize) -> Result<usize, ()> {
    if align.is_power_of_two() {
        Ok((unrounded + align - 1) & !(align - 1))
    } else {
        Err(())
    }
}

/// Reads an offset (`usize`) from `reader` whose encoding is described by `encoding`.
///
/// `encoding` must be a [DWARF Exception Header Encoding as described by the LSB spec][LSB-dwarf-ext].
/// In addition the upper ("application") part must be zero.
///
/// # Errors
/// Returns `Err` if `encoding`
/// * is not a valid DWARF Exception Header Encoding,
/// * is `DW_EH_PE_omit`, or
/// * has a non-zero application part.
///
/// [LSB-dwarf-ext]: https://refspecs.linuxfoundation.org/LSB_5.0.0/LSB-Core-generic/LSB-Core-generic/dwarfext.html
unsafe fn read_encoded_offset(reader: &mut DwarfReader, encoding: u8) -> Result<usize, ()> {
    if encoding == DW_EH_PE_omit || encoding & 0xF0 != 0 {
        return Err(());
    }
    let result = unsafe {
        match encoding & 0x0F {
            // despite the name, LLVM also uses absptr for offsets instead of pointers
            DW_EH_PE_absptr => reader.read::<usize>(),
            DW_EH_PE_uleb128 => reader.read_uleb128() as usize,
            DW_EH_PE_udata2 => reader.read::<u16>() as usize,
            DW_EH_PE_udata4 => reader.read::<u32>() as usize,
            DW_EH_PE_udata8 => reader.read::<u64>() as usize,
            DW_EH_PE_sleb128 => reader.read_sleb128() as usize,
            DW_EH_PE_sdata2 => reader.read::<i16>() as usize,
            DW_EH_PE_sdata4 => reader.read::<i32>() as usize,
            DW_EH_PE_sdata8 => reader.read::<i64>() as usize,
            _ => return Err(()),
        }
    };
    Ok(result)
}

/// Reads a pointer from `reader` whose encoding is described by `encoding`.
///
/// `encoding` must be a [DWARF Exception Header Encoding as described by the LSB spec][LSB-dwarf-ext].
///
/// # Errors
/// Returns `Err` if `encoding`
/// * is not a valid DWARF Exception Header Encoding,
/// * is `DW_EH_PE_omit`, or
/// * combines `DW_EH_PE_absptr` or `DW_EH_PE_aligned` application part with an integer encoding
///   (not `DW_EH_PE_absptr`) in the value format part.
///
/// [LSB-dwarf-ext]: https://refspecs.linuxfoundation.org/LSB_5.0.0/LSB-Core-generic/LSB-Core-generic/dwarfext.html
unsafe fn read_encoded_pointer(
    reader: &mut DwarfReader,
    context: &EHContext<'_>,
    encoding: u8,
) -> Result<*const u8, ()> {
    if encoding == DW_EH_PE_omit {
        return Err(());
    }

    log!("(pers) About to read encoded pointer at {:?}", reader.ptr);

    let base_ptr = match encoding & 0x70 {
        DW_EH_PE_absptr => {
            log!("(pers) encoding is: DW_EH_PE_absptr ({DW_EH_PE_absptr})");
            core::ptr::null()
        }
        // relative to address of the encoded value, despite the name
        DW_EH_PE_pcrel => {
            log!("(pers) encoding is: DW_EH_PE_pcrel ({DW_EH_PE_pcrel})");
            reader.ptr
        }
        DW_EH_PE_funcrel => {
            log!("(pers) encoding is: DW_EH_PE_funcrel ({DW_EH_PE_funcrel})");
            if context.func_start.is_null() {
                return Err(());
            }
            context.func_start
        }
        DW_EH_PE_textrel => {
            log!("(pers) encoding is: DW_EH_PE_textrel ({DW_EH_PE_textrel})");
            (*context.get_text_start)()
        }
        DW_EH_PE_datarel => {
            log!("(pers) encoding is: DW_EH_PE_textrel ({DW_EH_PE_datarel})");

            (*context.get_data_start)()
        }
        // aligned means the value is aligned to the size of a pointer
        DW_EH_PE_aligned => {
            log!("(pers) encoding is: DW_EH_PE_textrel ({DW_EH_PE_aligned})");
            reader.ptr = {
                let this = reader.ptr;
                let addr = round_up(
                    {
                        let this = reader.ptr;
                        unsafe { mem::transmute::<*const (), usize>(this.cast::<()>()) }
                    },
                    mem::size_of::<*const u8>(),
                )?;
                // In the mean-time, this operation is defined to be "as if" it was
                // a wrapping_offset, so we can emulate it as such. This should properly
                // restore pointer provenance even under today's compiler.
                let self_addr = unsafe { mem::transmute::<*const (), isize>(this.cast::<()>()) };
                let dest_addr = addr as isize;
                let offset = dest_addr.wrapping_sub(self_addr);

                // This is the canonical desugaring of this operation
                this.wrapping_byte_offset(offset)
            };
            core::ptr::null()
        }
        _ => return Err(()),
    };

    let mut ptr = if base_ptr.is_null() {
        // any value encoding other than absptr would be nonsensical here;
        // there would be no source of pointer provenance
        if encoding & 0x0F != DW_EH_PE_absptr {
            return Err(());
        }
        unsafe { reader.read::<*const u8>() }
    } else {
        log!("(pers) since base_ptr is not null, we must an offset");
        let offset = unsafe { read_encoded_offset(reader, encoding & 0x0F)? };
        log!("(pers) read offset is {offset:x?}");
        base_ptr.wrapping_add(offset)
    };

    log!("(pers) about to read from {ptr:?}");

    if encoding & DW_EH_PE_indirect != 0 {
        ptr = unsafe { ptr.cast::<*const u8>().read_unaligned() };
    }

    log!("(pers) returning ptr value {ptr:?}");

    Ok(ptr)
}
