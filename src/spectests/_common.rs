use crate::webassembly::ImportObject;

extern fn print_i32(num: i32) {
    println!("{}", num);
}

pub fn spectest_importobject<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("spectest", "print_i32", print_i32 as *const u8);
    return import_object
}
