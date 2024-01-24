// Allowed because it makes code more readable.
#![allow(clippy::bool_comparison, clippy::match_like_matches_macro)]

fn main() -> Result<(), anyhow::Error> {
    wasmer_backend_cli::run()
}
