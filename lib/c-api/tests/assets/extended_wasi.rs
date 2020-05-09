extern "C" {
    fn host_print(ptr: u32, len: u32);
}

fn main() {
    let args = std::env::args().collect::<Vec<String>>();

    println!("Found {} args on program {}", args.len(), args[0]);

    let env_vars = std::env::vars()
        .map(|(arg, val)| format!("{}={}", arg, val))
        .collect::<Vec<String>>();
    let env_var_list = env_vars.join(", ");

    println!("Found {} env vars: {}", env_vars.len(), env_var_list);

    let dirs_in_root = std::fs::read_dir("/")
        .unwrap()
        .map(|e| e.map(|inner| format!("{:?}", inner)))
        .collect::<Result<Vec<String>, _>>()
        .unwrap();

    println!(
        "Found {} pre opened dirs: {}",
        dirs_in_root.len(),
        dirs_in_root.join(", ")
    );

    const HOST_STR: &str = "This string came from a WASI module";
    unsafe { host_print(HOST_STR.as_ptr() as u32, HOST_STR.len() as u32) };
}
