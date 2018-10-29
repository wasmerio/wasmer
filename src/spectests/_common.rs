use crate::webassembly::ImportObject;

extern "C" fn print_i32(num: i32) {
    println!("{}", num);
}

extern "C" fn print() {}

static GLOBAL_I32: i32 = 666;

pub fn spectest_importobject<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("spectest", "print_i32", print_i32 as *const u8);
    import_object.set("spectest", "print", print as *const u8);
    import_object.set("spectest", "global_i32", GLOBAL_I32 as *const u8);
    return import_object;
}
