//! Utility functions for the webassembly library
use super::instance::Instance;
use std::mem::transmute;
use super::super::common::slice::{UncheckedSlice, BoundedSlice};

/// Detect if a provided binary is a WASM file
pub fn is_wasm_binary(binary: &Vec<u8>) -> bool {
    binary.starts_with(&[b'\0', b'a', b's', b'm'])
}

pub fn print_instance_offsets(instance: &Instance) {
    let instance_address = instance as *const _ as usize;

    let tables_pointer_address_ptr: *const usize =
        unsafe { transmute(&instance.data_pointers.tables) };
    let tables_pointer_address = tables_pointer_address_ptr as usize;

    let memories_pointer_address_ptr: *const usize =
        unsafe { transmute(&instance.data_pointers.memories) };
    let memories_pointer_address = memories_pointer_address_ptr as usize;

    let globals_pointer_address_ptr: *const usize =
        unsafe { transmute(&instance.data_pointers.globals) };
    let globals_pointer_address = globals_pointer_address_ptr as usize;

    let default_memory_bound_address_ptr: *const usize =
        unsafe { transmute(&instance.default_memory_bound) };
    let default_memory_bound_address = default_memory_bound_address_ptr as usize;

    println!(
        "
====== INSTANCE OFFSET TABLE ======
instance \t\t\t- {:X} | offset - {:?}
instance.data_pointers.tables \t- {:X} | offset - {:?}
instance.data_pointers.memories - {:X} | offset - {:?}
instance.data_pointers.globals \t- {:X} | offset - {:?}
instance.default_memory_bound \t- {:X} | offset - {:?}
====== INSTANCE OFFSET TABLE ======
        ",
        instance_address, 0,
        tables_pointer_address, tables_pointer_address - instance_address,
        memories_pointer_address, memories_pointer_address - instance_address,
        globals_pointer_address, globals_pointer_address - instance_address,
        default_memory_bound_address, default_memory_bound_address - instance_address,
    );
}
