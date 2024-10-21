fn main() {
    std::thread::spawn(|| {
        println!("In child thread");
    })
    .join()
    .unwrap();

    println!("In main thread");

    std::process::exit(0);
}
