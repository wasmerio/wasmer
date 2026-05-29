//#DefaultMappedDirectories: false
//#CurrentDirectory: /
//#Stdin: Hello, world!
//#ExpectedStdoutFile: pipe_reverse.stdout

use std::io;

fn main() {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let output: String = input.chars().rev().collect();

    println!("{}", output);
}
