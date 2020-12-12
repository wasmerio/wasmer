// WASI:
// stdin: "Hello, world!"

use std::io;

fn main() {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let output: String = input.chars().rev().collect();

    println!("{}", output);
}
