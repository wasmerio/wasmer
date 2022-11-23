fn main() {
    let compilers = std::env::var("COMPILERS").unwrap();
    println!("build wasmer, compilers = {compilers}");
}
