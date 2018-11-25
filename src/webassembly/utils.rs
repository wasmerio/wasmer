//! Utility functions for the webassembly library
use super::instance::Instance;
use std::mem::transmute;

/// Detect if a provided binary is a WASM file
pub fn is_wasm_binary(binary: &Vec<u8>) -> bool {
    binary.starts_with(&[b'\0', b'a', b's', b'm'])
}

pub fn print_instance_offsets(instance: &Instance) {
    let instance_address = instance as *const _ as usize;
    let data_ptr = &instance.data_pointers;

    let tables_pointer_address_ptr: *const usize = unsafe { transmute(&data_ptr.tables) };
    let tables_pointer_address = tables_pointer_address_ptr as usize;

    let memories_pointer_address_ptr: *const usize = unsafe { transmute(&data_ptr.memories) };
    let memories_pointer_address = memories_pointer_address_ptr as usize;

    let memories_pointer_address_ptr_0: *const usize =
        unsafe { transmute(&data_ptr.memories.get_unchecked(0)) };
    let memories_pointer_address_0 = memories_pointer_address_ptr_0 as usize;

    let memories_pointer_address_ptr_0_data: *const usize =
        unsafe { transmute(&data_ptr.memories.get_unchecked(0)) };
    let memories_pointer_address_0_data = memories_pointer_address_ptr_0_data as usize;

    let globals_pointer_address_ptr: *const usize = unsafe { transmute(&data_ptr.globals) };
    let globals_pointer_address = globals_pointer_address_ptr as usize;

    println!(
        "
====== INSTANCE OFFSET TABLE ======
instance \t\t\t- {:X} | offset - {:?}
instance.data_pointers.tables \t- {:X} | offset - {:?}
instance.data_pointers.memories\t- {:X} | offset - {:?}
    .memories[0] \t\t- {:X} | offset - {:?}
    .memories[0].data\t\t- {:X} | offset - {:?}
instance.data_pointers.globals \t- {:X} | offset - {:?}
====== INSTANCE OFFSET TABLE ======
        ",
        instance_address,
        0,
        tables_pointer_address,
        tables_pointer_address - instance_address,
        memories_pointer_address,
        memories_pointer_address - instance_address,
        memories_pointer_address_0,
        0,
        memories_pointer_address_0_data,
        memories_pointer_address_0_data - memories_pointer_address_0_data,
        globals_pointer_address,
        globals_pointer_address - instance_address,
    );
}
