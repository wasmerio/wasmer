fn main() {
    let compilers = std::env::var("COMPILERS").unwrap();
    println!("test capi, compilers = {compilers}");
}
