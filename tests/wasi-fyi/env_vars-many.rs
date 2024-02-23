use std::collections::HashMap;
use std::env;

fn main() {
    let vars = env::vars().collect::<HashMap<String, String>>();
    assert_eq!(vars.len(), 3);
    assert_eq!(vars["NONE"], "none".to_string());
    assert_eq!(vars["SOME"], "some".to_string());
    assert_eq!(vars["MANY"], "many".to_string());
}
