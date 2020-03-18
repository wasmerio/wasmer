fn main() {
    #[cfg(target_os = "windows")]
    {
        use cmake::Config;
        let project_name = "exception_handling";
        let dst = Config::new(project_name).build();
        println!("cargo:rustc-link-search=native={}", dst.display());
        println!("cargo:rustc-link-lib=static={}", project_name);
    }
    cc::Build::new()
        .include("exception_handling")
        .file("exception_handling/exception_handling.c")
        .compile("exception_handling");
}
