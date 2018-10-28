use crate::webassembly::{ImportObject, VmCtx};
use libc::putchar;

fn printf(format: i32, values: i32, context: &VmCtx) -> i32 {
    println!("PRINTF {:?} {:?}", format, values);
    return 0;
}

pub fn generate_libc_env<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("env", "printf", printf as *const u8);
    import_object.set("env", "putchar", putchar as *const u8);
    import_object
}

#[cfg(test)]
mod tests {
    use super::generate_libc_env;
    use crate::webassembly::{
        instantiate, ErrorKind, Export, ImportObject, Instance, Module, ResultObject, VmCtx,
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
        let context = instance.generate_context();
        main(&context);
    }

    // #[test]
    // fn test_printf() {
    //     let wasm_bytes = include_wast2wasm_bytes!("tests/printf.wast");
    //     let import_object = generate_libc_env();
    //     let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
    //     let module = result_object.module;
    //     let mut instance = result_object.instance;
    //     let func_index = match module.info.exports.get("main") {
    //         Some(&Export::Function(index)) => index,
    //         _ => panic!("Function not found"),
    //     };
    //     let main: fn(&VmCtx) = get_instance_function!(instance, func_index);
    //     let context = instance.generate_context();
    //     main(&context);
    // }
}
