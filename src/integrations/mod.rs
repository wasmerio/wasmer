use crate::webassembly::{ImportObject, VmCtx};
use libc::putchar;

// extern fn putchar(a: *const u8, context: *const u8) {
//     println!("PUT CHAAAR original pointer {:?}", context);
//     let vmctx: &VmCtx = unsafe { &*(context as *const VmCtx) };
//     println!("PUT CHAAAR {}", vmctx.test);
//     println!("PUT CHAAAR pointer {:p}", vmctx);
//     let x = vmctx as *const _;
//     let x_tables = vmctx.tables.as_ptr();
//     let tables_ptr_1 =  (&vmctx.tables) as *const _;
//     let tables_ptr_2 =  unsafe { (&vmctx.tables.get_unchecked(0)) as *const _ };
//     let tables_ptr_3 =  &vmctx.tables as *const _ ;
//     let tables_ptr_4 =  &vmctx.tables as *const _ ;
//     // let tables: &Vec<Vec<usize>> = unsafe { &*(tables_ptr_4 as *const Vec<Vec<usize>>) };
//     let x_tables_serial: &Vec<*const usize> = unsafe { &*(tables_ptr_1 as *const Vec<*const usize>) };
//     // let tables: &Vec<> = vmctx.tables as &Vec<Vec<usize>>;
//     println!("PUT CHAAAR pointer {:?}", x);
//     println!("PUT CHAAAR pointer 1 {:p}", &vmctx.tables);
//     println!("PUT CHAAAR pointer 2 {:p}", tables_ptr_1);
//     println!("PUT CHAAAR pointer 3 (0) {:p}", tables_ptr_2);
//     println!("PUT CHAAAR pointer 3 (0) {:p}", tables_ptr_3);
//     // println!("PUT CHAAAR pointer 4 (0) {:p} {:?}", tables_ptr_4, tables);
//     // println!("PUT CHAAAR pointer 4 (0) {:p} {:?}", &vmctx.tables, vmctx.tables);
//     // println!("PUT CHAAAR pointer 4 (0) {:p} {:?}", &vmctx.tables[0], vmctx.tables[0]);
//     println!("PUT CHAAAR pointer {:?} {:?}", x_tables, x_tables_serial);
//     let x_tables = vmctx.tables.as_ptr();
//     println!("PUT CHAAAR pointer {:?}", x_tables);
// }

pub fn generate_libc_env<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("env", "putchar", putchar as *const u8);
    import_object
}

#[cfg(test)]
mod tests {
    use super::generate_libc_env;
    use crate::webassembly::{
        instantiate, ErrorKind, Export, ImportObject, Instance, Module, ResultObject,
        VmCtx
    };
    use libc::putchar;

    #[test]
    fn test_putchar() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/putchar.wast");
        let import_object = generate_libc_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        let module = result_object.module;
        let mut instance = result_object.instance;
        let func_index = match module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn(&VmCtx) = get_instance_function!(instance, func_index);
        let mainn_func_index = match module.info.exports.get("mainn") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let mainn: fn(&VmCtx) = get_instance_function!(instance, mainn_func_index);
        let context = instance.generate_context();
        main(&context);
        println!("---------MAINNN NOW---------");
        // let context = instance.generate_context();
        mainn(&context);
    }
}
