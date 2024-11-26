fn main() {
    std::thread::spawn(|| std::process::exit(1)).join().unwrap();
}
