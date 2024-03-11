use std::env;

fn main() {
  let args = env::args().collect::<Vec<String>>();
  assert_eq!(args.len(), 4);
  assert_eq!(args[0], "env_args-many.wasm");
  assert_eq!(args[1], "none");
  assert_eq!(args[2], "some");
  assert_eq!(args[3], "many");
}
