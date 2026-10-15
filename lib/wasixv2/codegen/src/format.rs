//! Output formatting.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Formats Rust source with `rustfmt --edition 2024` (`$RUSTFMT` overrides the binary).
///
/// Checked-in output must pass `cargo fmt --check`, so we use the same tool rather than a
/// pretty-printer that could disagree with it. rustfmt looks up config from its working
/// directory when reading stdin; pinning that to this crate (as `cargo fmt` would see the repo)
/// makes the output independent of the caller's CWD.
pub fn rustfmt(src: &str) -> Result<String> {
    let bin = std::env::var("RUSTFMT").unwrap_or_else(|_| "rustfmt".to_string());
    let mut child = Command::new(&bin)
        .args(["--edition", "2024", "--emit", "stdout"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run `{bin}`"))?;
    let mut stdin = child.stdin.take().context("rustfmt stdin")?;
    let input = src.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("rustfmt writer panicked"))??;
    if !output.status.success() {
        bail!(
            "rustfmt failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}
