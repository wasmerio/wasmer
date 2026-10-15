use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use wasixv2_codegen::Target;

/// Generates wasixv2 bindings from the annotated WIT definitions.
///
/// Run from the repository root: the `--wit` path is recorded (relative) in the output header
/// so the up-to-date tests can regenerate from the same source.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

/// Where a Rust backend reads from and writes to.
#[derive(clap::Args)]
struct RustArgs {
    /// WIT file or directory (relative to the current directory).
    #[arg(long)]
    wit: PathBuf,
    /// Output `.rs` file.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate `lib/wasixv2/abi/src/generated.rs`.
    Abi(RustArgs),
    /// Generate the libc2 (musl) syscall glue.
    #[command(group(clap::ArgGroup::new("dest").required(true)))]
    C {
        /// WIT file or directory.
        #[arg(long)]
        wit: PathBuf,
        /// libc2 checkout: overwrites the generated files in place.
        #[arg(long, group = "dest")]
        libc: Option<PathBuf>,
        /// Directory to write the files to, mirroring the libc2 tree layout.
        #[arg(long, group = "dest")]
        out_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Abi(args) => write_rust(args, wasixv2_codegen::generate_abi),
        Cmd::C { wit, libc, out_dir } => {
            let files = wasixv2_codegen::generate_c(&wit, Target::WASM32)?;
            let (root, create) = match (libc, out_dir) {
                (Some(libc), _) => {
                    // Catch a wrong path before scattering files into it.
                    if !libc.join("arch/wasm32/syscall_arch.h").is_file() {
                        bail!("{} does not look like a libc2 checkout", libc.display());
                    }
                    (libc, false)
                }
                (None, Some(dir)) => (dir, true),
                (None, None) => unreachable!("clap requires one destination"),
            };
            for f in files {
                let path = root.join(f.path);
                if create && let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                std::fs::write(&path, f.contents)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            Ok(())
        }
    }
}

fn write_rust(args: RustArgs, generate: fn(&Path, &str, Target) -> Result<String>) -> Result<()> {
    let source = source_label(&args.wit)?;
    let code = generate(&args.wit, &source, Target::WASM32)?;
    std::fs::write(&args.out, code).with_context(|| format!("writing {}", args.out.display()))
}

fn source_label(wit: &Path) -> Result<String> {
    if wit.is_absolute() {
        bail!("--wit must be relative to the repository root");
    }
    let parts: Vec<_> = wit.iter().map(|c| c.to_string_lossy()).collect();
    Ok(parts.join("/"))
}
