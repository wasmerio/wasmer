use anyhow::Error;
use wasix_conformance_suite_runner::Resolver;
use wasmer::Cranelift;

fn main() -> Result<(), Error> {
    let args = libtest_mimic::Arguments::from_args();

    let compiler = Cranelift::new();
    let tests = Resolver::new().resolve(compiler.into())?;

    libtest_mimic::run(&args, tests).exit();
}
