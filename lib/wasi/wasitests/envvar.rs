// Args:
// env: DOG=1
// env: CAT=2

use std::env;

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
}
