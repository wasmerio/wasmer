fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("TARGET_OS not specified") != "windows" {
        return;
    }

    cc::Build::new()
        .include("exception_handling")
        .file("exception_handling/exception_handling.c")
        .compile("exception_handling");
}
