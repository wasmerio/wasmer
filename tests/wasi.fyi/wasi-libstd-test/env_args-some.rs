use std::env;

fn main() {
  let args = env::args().collect::<Vec<String>>();
  assert_eq!(args.len(), 2);
  assert_eq!(args[0], "env_args-some.wasm");
  assert_eq!(args[1], "some");
}
