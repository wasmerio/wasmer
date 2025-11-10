use chrono::prelude::*;

pub fn main() {
    let utc: DateTime<Utc> = Utc::now();
    let date = utc.format("%Y-%m-%d").to_string();
    println!("cargo:rustc-env=WASMER_BUILD_DATE={date}");
}
