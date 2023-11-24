use std::env;

fn main() {
  let vars = env::vars().collect::<Vec<(String, String)>>();
  assert_eq!(vars.len(), 3);
  assert_eq!(vars[0], ("NONE".to_string(), "none".to_string()));
  assert_eq!(vars[1], ("SOME".to_string(), "some".to_string()));
  assert_eq!(vars[2], ("MANY".to_string(), "many".to_string()));
}
