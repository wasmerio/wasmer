use chrono::prelude::*;

pub fn main() {
    let utc: DateTime<Utc> = Utc::now();
    let date = utc.format("%Y-%m-%d").to_string();
    println!("cargo:rustc-env=WASMER_BUILD_DATE={date}");

    let reproducible_build = std::env::var("WASMER_REPRODUCIBLE_BUILD")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true"));
    println!("cargo:rustc-env=WASMER_REPRODUCIBLE_BUILD={reproducible_build}");
}
