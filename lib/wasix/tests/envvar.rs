// WASI:
// env: DOG=1
// env: CAT=2

use std::env;

fn get_env_var(var_name: &str) -> Result<String, env::VarError> {
    env::var(var_name)
}

fn main() {
    let mut env_vars = env::vars()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<String>>();

    env_vars.sort();

    println!("Env vars:");
    for e in env_vars {
        println!("{e}");
    }

    env::set_var("WASI_ENVVAR_TEST", "HELLO");

    println!("DOG {:?}", get_env_var("DOG"));
    println!("DOG_TYPE {:?}", get_env_var("DOG_TYPE"));
    println!("SET VAR {:?}", env::var("WASI_ENVVAR_TEST"));
}
