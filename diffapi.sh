rustup update nightly
cargo uninstall cargo-public-api
cargo install cargo-public-api --git https://github.com/Enselic/cargo-public-api
cargo +nightly rustdoc -p wasmer --lib -- -Z unstable-options --output-format json
cargo +nightly rustdoc -p wasmer --no-default-features --features="js,std" --target wasm32-wasi --lib -- -Z unstable-options --output-format json
cp target/doc/wasmer.json ./wasmer-sys.json
cp target/wasm32-wasi/doc/wasmer.json ./wasmer-js.json
~/Development/cargo-public-api/target/release/public-api ./wasmer-js.json ./wasmer-sys.json > api.diff