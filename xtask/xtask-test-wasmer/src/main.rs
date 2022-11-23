fn main() {
    let compilers = std::env::var("COMPILERS").unwrap();
    println!("test wasmer, compilers = {compilers}");
}
