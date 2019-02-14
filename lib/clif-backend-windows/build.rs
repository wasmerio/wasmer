use cmake::Config;

fn main() {
    #[cfg(target_os = "windows")]
    {
        let project_name = "exception_handling";
        let dst = Config::new(project_name).build();
        println!("cargo:rustc-link-search=native={}", dst.display());
        println!("cargo:rustc-link-lib=static={}", project_name);
    }
}
