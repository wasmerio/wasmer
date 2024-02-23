use std::env;

fn main() {
  let args = env::args().collect::<Vec<String>>();
  assert_eq!(args.len(), 1);
  assert_eq!(args[0], "env_args-none.wasm");
}
