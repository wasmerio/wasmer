// WASI:
// env: DOG=1
// env: CAT=2

use std::env;

fn get_env_var(var_name: &str) -> Result<String, env::VarError> {
    #[cfg(not(target = "wasi"))]
    match var_name {
        "DOG" => Ok("1".to_string()),
        "CAT" => Ok("2".to_string()),
        _ => Err(env::VarError::NotPresent),
    }
    #[cfg(target = "wasi")]
    env::var(var_name)
}

fn main() {
    #[cfg(not(target = "wasi"))]
    let mut env_vars = vec!["DOG=1".to_string(), "CAT=2".to_string()];
    #[cfg(target = "wasi")]
    let mut env_vars = env::vars()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<String>>();

    env_vars.sort();

    println!("Env vars:");
    for e in env_vars {
        println!("{}", e);
    }

    env::set_var("WASI_ENVVAR_TEST", "HELLO");

    println!("DOG {:?}", get_env_var("DOG"));
    println!("DOG_TYPE {:?}", get_env_var("DOG_TYPE"));
    println!("SET VAR {:?}", env::var("WASI_ENVVAR_TEST"));
}
