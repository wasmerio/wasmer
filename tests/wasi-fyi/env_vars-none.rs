use std::env;

fn main() {
  let vars = env::vars().collect::<Vec<(String, String)>>();
  assert_eq!(vars.len(), 0);
}
