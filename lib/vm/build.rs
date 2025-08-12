#[rustversion::since(1.89)]
fn main() {
    println!("cargo::rustc-cfg=missing_rust_probestack");
    println!("cargo::rustc-check-cfg=cfg(missing_rust_probestack)");
}

#[rustversion::before(1.89)]
fn main() {
    println!("cargo::rustc-check-cfg=cfg(missing_rust_probestack)");
}
