//! The commands available in the Wasmer binary.
mod add;
mod app;
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
mod connect;
mod container;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
mod create_exe;
#[cfg(feature = "static-artifact-create")]
mod create_obj;
pub(crate) mod deploy;
pub(crate) mod domain;
#[cfg(feature = "static-artifact-create")]
mod gen_c_header;
mod init;
mod inspect;
#[cfg(feature = "journal")]
mod journal;
mod login;
pub(crate) mod namespace;
mod package;
mod publish;
mod run;
mod self_update;
pub mod ssh;
mod validate;
#[cfg(feature = "wast")]
mod wast;
mod whoami;

#[cfg(target_os = "linux")]
pub use binfmt::*;
use clap::{CommandFactory, Parser};
#[cfg(feature = "compiler")]
pub use compile::*;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
#[cfg(feature = "static-artifact-create")]
pub use {create_obj::*, gen_c_header::*};

#[cfg(feature = "journal")]
pub use self::journal::*;
pub use self::{
    add::*, cache::*, config::*, container::*, init::*, inspect::*, login::*, package::*,
    publish::*, run::Run, self_update::*, validate::*, whoami::*,
};
use crate::error::PrettyError;

/// An executable CLI command.
pub(crate) trait CliCommand {
    type Output;

    fn run(self) -> Result<(), anyhow::Error>;
}

/// An executable CLI command that runs in an async context.
///
/// An [`AsyncCliCommand`] automatically implements [`CliCommand`] by creating
/// a new tokio runtime and blocking.
#[async_trait::async_trait]
pub(crate) trait AsyncCliCommand: Send + Sync {
    type Output: Send + Sync;

    async fn run_async(self) -> Result<Self::Output, anyhow::Error>;
}

impl<O: Send + Sync, C: AsyncCliCommand<Output = O>> CliCommand for C {
    type Output = O;

    fn run(self) -> Result<(), anyhow::Error> {
        tokio::runtime::Runtime::new()?.block_on(AsyncCliCommand::run_async(self))?;
        Ok(())
    }
}

/// Command-line arguments for the Wasmer CLI.
#[derive(clap::Parser, Debug)]
#[clap(author, version)]
#[clap(disable_version_flag = true)] // handled manually
#[cfg_attr(feature = "headless", clap(
    name = "wasmer-headless",
    about = concat!("wasmer-headless ", env!("CARGO_PKG_VERSION")),
))]
#[cfg_attr(not(feature = "headless"), clap(
    name = "wasmer",
    about = concat!("wasmer ", env!("CARGO_PKG_VERSION")),
))]
pub struct WasmerCmd {
    /// Print version info and exit.
    #[clap(short = 'V', long)]
    version: bool,
    #[clap(flatten)]
    output: crate::logging::Output,
    #[clap(subcommand)]
    cmd: Option<Cmd>,
}

impl WasmerCmd {
    fn execute(self) -> Result<(), anyhow::Error> {
        let WasmerCmd {
            cmd,
            version,
            output,
        } = self;

        output.initialize_logging();

        if version {
            return print_version(output.is_verbose());
        }

        match cmd {
            Some(Cmd::Run(options)) => options.execute(output),
            Some(Cmd::SelfUpdate(options)) => options.execute(),
            Some(Cmd::Cache(cache)) => cache.execute(),
            Some(Cmd::Validate(validate)) => validate.execute(),
            #[cfg(feature = "compiler")]
            Some(Cmd::Compile(compile)) => compile.execute(),
            #[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
            Some(Cmd::CreateExe(create_exe)) => create_exe.execute(),
            #[cfg(feature = "static-artifact-create")]
            Some(Cmd::CreateObj(create_obj)) => create_obj.execute(),
            Some(Cmd::Config(config)) => config.execute(),
            Some(Cmd::Inspect(inspect)) => inspect.execute(),
            Some(Cmd::Init(init)) => init.execute(),
            Some(Cmd::Login(login)) => login.execute(),
            Some(Cmd::Publish(publish)) => publish.execute(),
            Some(Cmd::Package(cmd)) => match cmd {
                Package::Download(cmd) => cmd.execute(),
                Package::Build(cmd) => cmd.execute(),
            },
            Some(Cmd::Container(cmd)) => match cmd {
                crate::commands::Container::Unpack(cmd) => cmd.execute(),
            },
            /*
            Some(Cmd::Connect(connect)) => connect.execute(),
            */
            #[cfg(feature = "static-artifact-create")]
            Some(Cmd::GenCHeader(gen_heder)) => gen_heder.execute(),
            #[cfg(feature = "wast")]
            Some(Cmd::Wast(wast)) => wast.execute(),
            #[cfg(target_os = "linux")]
            Some(Cmd::Binfmt(binfmt)) => binfmt.execute(),
            Some(Cmd::Whoami(whoami)) => whoami.execute(),
            Some(Cmd::Add(install)) => install.execute(),

            // Deploy commands.
            Some(Cmd::Deploy(c)) => c.run(),
            Some(Cmd::App(apps)) => apps.run(),
            #[cfg(feature = "journal")]
            Some(Cmd::Journal(journal)) => journal.run(),
            Some(Cmd::Ssh(ssh)) => ssh.run(),
            Some(Cmd::Namespace(namespace)) => namespace.run(),
            Some(Cmd::Domain(namespace)) => namespace.run(),
            None => {
                WasmerCmd::command().print_long_help()?;
                // Note: clap uses an exit code of 2 when CLI parsing fails
                std::process::exit(2);
            }
        }
    }

    /// The main function for the Wasmer CLI tool.
    pub fn run() {
        // We allow windows to print properly colors
        #[cfg(windows)]
        colored::control::set_virtual_terminal(true).unwrap();

        PrettyError::report(Self::run_inner())
    }

    fn run_inner() -> Result<(), anyhow::Error> {
        if is_binfmt_interpreter() {
            Run::from_binfmt_args().execute(crate::logging::Output::default());
        }

        match WasmerCmd::try_parse() {
            Ok(args) => args.execute(),
            Err(e) => {
                let might_be_wasmer_run = matches!(
                    e.kind(),
                    clap::error::ErrorKind::InvalidSubcommand
                        | clap::error::ErrorKind::UnknownArgument
                );

                if might_be_wasmer_run {
                    if let Ok(run) = Run::try_parse() {
                        // Try to parse the command using the `wasmer some/package`
                        // shorthand. Note that this has discoverability issues
                        // because it's not shown as part of the main argument
                        // parser's help, but that's fine.
                        let output = crate::logging::Output::default();
                        output.initialize_logging();
                        run.execute(output);
                    }
                }

                e.exit();
            }
        }
    }
}

#[derive(clap::Parser, Debug)]
#[allow(clippy::large_enum_variant)]
/// The options for the wasmer Command Line Interface
enum Cmd {
    /// Login into a wasmer.io-like registry
    Login(Login),

    /// Login into a wasmer.io-like registry
    #[clap(name = "publish")]
    Publish(Publish),

    /// Wasmer cache
    Cache(Cache),

    /// Validate a WebAssembly binary
    Validate(Validate),

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    Compile(Compile),

    /// Compile a WebAssembly binary into a native executable
    ///
    /// To use, you need to set the `WASMER_DIR` environment variable
    /// to the location of your Wasmer installation. This will probably be `~/.wasmer`. It
    /// should include a `lib`, `include` and `bin` subdirectories. To create an executable
    /// you will need `libwasmer`, so by setting `WASMER_DIR` the CLI knows where to look for
    /// header files and libraries.
    ///
    /// Example usage:
    ///
    /// ```text
    /// $ # in two lines:
    /// $ export WASMER_DIR=/home/user/.wasmer/
    /// $ wasmer create-exe qjs.wasm -o qjs.exe # or in one line:
    /// $ WASMER_DIR=/home/user/.wasmer/ wasmer create-exe qjs.wasm -o qjs.exe
    /// $ file qjs.exe
    /// qjs.exe: ELF 64-bit LSB pie executable, x86-64 ...
    /// ```
    ///
    /// ## Cross-compilation
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    #[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
    #[clap(name = "create-exe", verbatim_doc_comment)]
    CreateExe(CreateExe),

    /// Compile a WebAssembly binary into an object file
    ///
    /// To use, you need to set the `WASMER_DIR` environment variable to the location of your
    /// Wasmer installation. This will probably be `~/.wasmer`. It should include a `lib`,
    /// `include` and `bin` subdirectories. To create an object you will need `libwasmer`, so by
    /// setting `WASMER_DIR` the CLI knows where to look for header files and libraries.
    ///
    /// Example usage:
    ///
    /// ```text
    /// $ # in two lines:
    /// $ export WASMER_DIR=/home/user/.wasmer/
    /// $ wasmer create-obj qjs.wasm --object-format symbols -o qjs.obj # or in one line:
    /// $ WASMER_DIR=/home/user/.wasmer/ wasmer create-exe qjs.wasm --object-format symbols -o qjs.obj
    /// $ file qjs.obj
    /// qjs.obj: ELF 64-bit LSB relocatable, x86-64 ...
    /// ```
    ///
    /// ## Cross-compilation
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    #[cfg(feature = "static-artifact-create")]
    #[structopt(name = "create-obj", verbatim_doc_comment)]
    CreateObj(CreateObj),

    /// Generate the C static_defs.h header file for the input .wasm module
    #[cfg(feature = "static-artifact-create")]
    GenCHeader(GenCHeader),

    /// Get various configuration information needed
    /// to compile programs which use Wasmer
    Config(Config),

    /// Update wasmer to the latest version
    #[clap(name = "self-update")]
    SelfUpdate(SelfUpdate),

    /// Inspect a WebAssembly file
    Inspect(Inspect),

    /// Initializes a new wasmer.toml file
    #[clap(name = "init")]
    Init(Init),

    /// Run spec testsuite
    #[cfg(feature = "wast")]
    Wast(Wast),

    /// Unregister and/or register wasmer as binfmt interpreter
    #[cfg(target_os = "linux")]
    Binfmt(Binfmt),

    /// Shows the current logged in user for the current active registry
    Whoami(Whoami),

    /// Add a Wasmer package's bindings to your application.
    Add(Add),

    /// Run a WebAssembly file or Wasmer container.
    #[clap(alias = "run-unstable")]
    Run(Run),

    /// Manage journals (compacting, inspecting, filtering, ...)
    #[cfg(feature = "journal")]
    #[clap(subcommand)]
    Journal(CmdJournal),

    #[clap(subcommand)]
    Package(crate::commands::Package),

    #[clap(subcommand)]
    Container(crate::commands::Container),

    // Edge commands
    /// Deploy apps to Wasmer Edge.
    Deploy(crate::commands::deploy::CmdDeploy),

    /// Manage deployed Edge apps.
    #[clap(subcommand, alias = "apps")]
    App(crate::commands::app::CmdApp),

    /// Run commands/packages on Wasmer Edge in an interactive shell session.
    Ssh(crate::commands::ssh::CmdSsh),

    /// Manage Wasmer namespaces.
    #[clap(subcommand, alias = "namespaces")]
    Namespace(crate::commands::namespace::CmdNamespace),

    /// Manage DNS records
    #[clap(subcommand, alias = "domains")]
    Domain(crate::commands::domain::CmdDomain),
}

fn is_binfmt_interpreter() -> bool {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "linux")] {
            // Note: we'll be invoked by the kernel as Binfmt::FILENAME
            let binary_path = match std::env::args_os().next() {
                Some(path) => std::path::PathBuf::from(path),
                None => return false,
            };
            binary_path.file_name().and_then(|f| f.to_str()) == Some(Binfmt::FILENAME)
        } else {
            false
        }
    }
}

fn print_version(verbose: bool) -> Result<(), anyhow::Error> {
    if !verbose {
        println!("wasmer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    println!(
        "wasmer {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("WASMER_BUILD_GIT_HASH_SHORT"),
        env!("WASMER_BUILD_DATE")
    );
    println!("binary: {}", env!("CARGO_PKG_NAME"));
    println!("commit-hash: {}", env!("WASMER_BUILD_GIT_HASH"));
    println!("commit-date: {}", env!("WASMER_BUILD_DATE"));
    println!("host: {}", target_lexicon::HOST);

    let mut compilers = Vec::<&'static str>::new();
    if cfg!(feature = "singlepass") {
        compilers.push("singlepass");
    }
    if cfg!(feature = "cranelift") {
        compilers.push("cranelift");
    }
    if cfg!(feature = "llvm") {
        compilers.push("llvm");
    }
    println!("compiler: {}", compilers.join(","));

    Ok(())
}
