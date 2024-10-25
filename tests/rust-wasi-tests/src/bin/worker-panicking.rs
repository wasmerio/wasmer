fn main() {
    std::thread::spawn(|| {
        panic!("child thread panicking");
    })
    .join()
    .unwrap();

    println!("In main thread");

    std::process::exit(0);
}
