fn main() {
    std::thread::spawn(|| std::process::exit(0)).join().unwrap();
}
