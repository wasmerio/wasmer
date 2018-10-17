use libc::putchar;

#[cfg(test)]
mod tests {
    use crate::webassembly::{
        instantiate, ErrorKind, Export, ImportObject, Instance, Module, ResultObject,
    };
    use libc::putchar;

    #[test]
    fn test_putchar() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/putchar.wast");
        let mut import_object = ImportObject::new();
        import_object.set("env", "putchar", putchar as *const u8);

        let result_object =
            instantiate(wasm_bytes, Some(import_object)).expect("Not compiled properly");
        let module = result_object.module;
        let instance = result_object.instance;
        let func_index = match module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn() = get_instance_function!(instance, func_index);
        main();
    }
}
