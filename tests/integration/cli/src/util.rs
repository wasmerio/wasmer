use anyhow::bail;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Copy, Clone)]
pub enum Compiler {
    Cranelift,
    LLVM,
    Singlepass,
}

impl Compiler {
    pub const fn to_flag(self) -> &'static str {
        match self {
            Compiler::Cranelift => "--cranelift",
            Compiler::LLVM => "--llvm",
            Compiler::Singlepass => "--singlepass",
        }
    }
}

pub fn run_code(
    operating_dir: &Path,
    executable_path: &Path,
    args: &[String],
    stderr: bool,
) -> anyhow::Result<String> {
    let output = Command::new(executable_path.canonicalize()?)
        .current_dir(operating_dir)
        .args(args)
        .output()?;

    if !output.status.success() && !stderr {
        bail!(
            "running executable failed: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    let output = std::str::from_utf8(if stderr {
        &output.stderr
    } else {
        &output.stdout
    })
    .expect("output from running executable is not utf-8");

    Ok(output.to_owned())
}
