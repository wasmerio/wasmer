use std::process::{Command, Stdio};

use rustc_version::{Channel, VersionMeta};

fn main() {
    if std::env::var("CARGO_FEATURE_JS").is_ok() {
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
        let mut cmd = Command::new(rustc);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(env!("CARGO_MANIFEST_DIR"));

        let VersionMeta {
            channel,
            short_version_string,
            ..
        } = VersionMeta::for_command(cmd).unwrap();

        if channel != Channel::Nightly {
            println!(
                "cargo:warning={} was compiled with {short_version_string}, \
                but the \"js\" feature requires nightly features. \
                See https://github.com/wasmerio/wasmer/issues/4132 for more context.",
                env!("CARGO_PKG_NAME"),
            );
        }
    }
}
