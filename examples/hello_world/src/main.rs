fn main() {
    for i in 0..8 {
        let s = format!("Hello, {}", i);
        println!("{}", s);
    }
    panic!("OK");
}
