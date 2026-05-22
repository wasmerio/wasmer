//! Tests that build and run various WASIX test programs.
//!
//! Primary test files can contain directives that define one or more configurations
//! for a WASM test. Each configuration represents a distinct test run, with its
//! own arguments, environment setup, expected exit status, and output/file checks.
//!
//! Directives use `//#Directive: Args` in C/C++ sources and `##Directive: Args` in
//! shell sources.
//!
//! Supported directives:
//!
//! `Config:{name}[:{inherits}]` starts a runnable configuration. When `inherits`
//! is present, the new configuration copies the named earlier configuration first.
//!
//! `AbstractConfig:{name}[:{inherits}]` starts a configuration that can be inherited
//! from but is not run directly.
//!
//! `Args:{args}` sets whitespace-separated command-line arguments.
//!
//! `BuildEnv:{key}={value}` sets an environment variable before building.
//!
//! `ExpectedStdout:{line}` appends one expected stdout line.
//! Can be used multiple times and all expected lines must match the trimmed stdout exactly.
//!
//! `MustFail:{bool}` requires a non-zero exit code when true.
//!
//! `ExpectedExitCode:{code}` sets the expected numeric exit code.
//!
//! `Ignored:{reason}` marks the configuration as ignored with the given reason.
//!
//! `SkipEngine:{engine}:{reason}` marks the configuration as ignored for
//! a given engine (LLVM, Cranelift, V8).
//!
//! `UnixOnly:{bool}` ignores the configuration on non-Unix hosts when true.
//!
//! `MappedDirectory:{host}:{guest}` maps a host directory into the guest. Relative
//!  host paths are resolved from the test source directory; `$temp` creates a fresh
//!  temporary host directory.
//!
//! `CurrentDirectory:{guest_path}` sets the guest current working directory.
//!
//! `PrefilledFile:{relative_path}:{contents}` writes a file before the test runs.
//!
//! `ExpectedFile:{relative_path}:{contents}` checks a file after the test runs.

use anyhow::{Context, Result, anyhow, ensure};
use itertools::Itertools;
use std::collections::HashMap;
use std::fs::{File, create_dir_all, read_dir, remove_dir_all};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::bail;
use libtest_mimic::Trial;
use walkdir::WalkDir;

mod runner;

fn should_emit_colour() -> bool {
    std::io::stdout().is_terminal()
        || std::env::var("CARGO_TERM_COLOR").as_deref() == Ok("always")
        || std::env::var("NEXTEST").is_ok()
}

fn main() -> Result<std::process::ExitCode> {
    let mut args = libtest_mimic::Arguments::from_args();
    if should_emit_colour() {
        args.color = Some(libtest_mimic::ColorSetting::Always);
    }
    let mut tests = Vec::new();
    collect_tests(&mut tests)?;
    Ok(libtest_mimic::run(&args, tests).exit_code())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostMappedLocation {
    TemporaryFolder,
    HostPath(String),
}

impl HostMappedLocation {
    fn new(path: &str) -> Self {
        if path == "$temp" {
            Self::TemporaryFolder
        } else {
            Self::HostPath(path.to_owned())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MappedDirectory {
    host: HostMappedLocation,
    guest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Cranelift,
    LLVM,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cranelift => "cranelift",
            Self::LLVM => "llvm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    /// The directory containing the test sources.
    source: PrimarySource,
    test_src_dir: PathBuf,
    tests_build_root: PathBuf,

    test_name: String,
    config_name: String,
    engine: Engine,
    is_abstract: bool,

    nonzero_exit_code: bool,
    expected_exit_code: i32,
    expected_stdout: Vec<String>,
    arguments: Vec<String>,
    build_env: Vec<(String, String)>,
    ignored: Option<String>,
    skipped_engines: Vec<(Engine, String)>,
    unix_only: bool,
    mapped_directories: Vec<MappedDirectory>,
    current_directory: Option<String>,
    prefilled_files: Vec<(PathBuf, String)>,
    expected_files: Vec<(PathBuf, String)>,
}

impl Config {
    fn new(
        source: PrimarySource,
        test_src_dir: PathBuf,
        tests_build_root: PathBuf,
        test_name: String,
    ) -> Self {
        Self {
            source,
            test_src_dir,
            tests_build_root,
            test_name,
            config_name: "default".to_owned(),
            engine: Engine::Cranelift,
            is_abstract: false,
            arguments: Vec::new(),
            build_env: Vec::new(),
            nonzero_exit_code: false,
            expected_exit_code: 0,
            expected_stdout: Vec::new(),
            ignored: None,
            skipped_engines: Vec::new(),
            unix_only: false,
            mapped_directories: Vec::new(),
            current_directory: None,
            prefilled_files: Vec::new(),
            expected_files: Vec::new(),
        }
    }

    fn build_path(&self) -> PathBuf {
        self.tests_build_root.join(self.full_test_name())
    }

    fn full_test_name(&self) -> String {
        if self.source.is_default() {
            format!(
                "wasm/{}/{}/{}",
                self.test_name,
                self.config_name,
                self.engine.name(),
            )
        } else {
            format!(
                "wasm/{}/{}/{}/{}",
                self.test_name,
                self.source.config_name(),
                self.config_name,
                self.engine.name(),
            )
        }
    }
}

fn parse_configs(default_config: &Config) -> Result<Vec<Config>> {
    let src_filename = default_config
        .test_src_dir
        .join(default_config.source.filename());
    let source = std::fs::read_to_string(&src_filename)
        .with_context(|| format!("Failed to read {}", src_filename.display()))?;

    let mut configs = Vec::new();
    let mut config_name_to_index = HashMap::new();
    let mut config = default_config.clone();
    let mut build_env = Vec::new();

    let directive_prefix = match src_filename
        .extension()
        .expect("extension expected")
        .to_str()
        .expect("must be valid string")
    {
        "c" | "cpp" => "//#",
        "sh" => "##",
        suffix => bail!("unexpected extension '{suffix}' of a primary source: {src_filename:?}"),
    };

    for (i, line) in source.lines().enumerate() {
        if let Some(rest) = line.trim().strip_prefix(directive_prefix) {
            process_directive(
                rest,
                &mut build_env,
                &mut config,
                default_config,
                &mut config_name_to_index,
                &mut configs,
            )
            .with_context(|| {
                format!(
                    "Failed to process test directive {}:{}",
                    src_filename.display(),
                    i + 1
                )
            })?;
        }
    }

    configs.push(config);

    for config in &mut configs {
        config.build_env = build_env.clone();
    }

    configs.retain(|c| !c.is_abstract);

    if configs.is_empty() {
        bail!("Missing non-abstract Config");
    }

    Ok(configs)
}

fn process_directive(
    rest: &str,
    build_env: &mut Vec<(String, String)>,
    config: &mut Config,
    default_config: &Config,
    config_name_to_index: &mut HashMap<String, usize>,
    configs: &mut Vec<Config>,
) -> Result<()> {
    let (directive, arg) = rest.split_once(':').context("Missing arg")?;
    let arg = arg.trim();
    match directive {
        "Config" | "AbstractConfig" => {
            if config != default_config {
                let index = configs.len();
                config_name_to_index.insert(config.config_name.clone(), index);
                configs.push(config.clone());
            }

            let name = if let Some((name, inherit)) = arg.split_once(':') {
                let inherit_index = config_name_to_index.get(inherit).ok_or_else(|| {
                    anyhow!("Config `{name}` inherits from unknown config named `{inherit}`")
                })?;

                *config = configs[*inherit_index].clone();
                name
            } else {
                *config = default_config.clone();
                arg
            };
            config.is_abstract = directive == "AbstractConfig";
            if config_name_to_index.contains_key(name) {
                bail!("Duplicate config `{name}`");
            }
            name.clone_into(&mut config.config_name);
        }
        "Args" => {
            config.arguments = arg
                .split(' ')
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
                .collect();
        }
        "ExpectedStdout" => {
            config.expected_stdout.push(arg.to_owned());
        }
        "BuildEnv" => {
            let (key, value) = arg
                .split_once('=')
                .ok_or_else(|| anyhow!("missing equals separator for BuildEnv"))?;
            let key = key.trim();
            ensure!(!key.is_empty(), "BuildEnv key must not be empty");
            build_env.push((key.to_owned(), value.trim().to_owned()));
        }
        "MustFail" => {
            config.nonzero_exit_code = arg.parse::<bool>()?;
        }
        "ExpectedExitCode" => {
            config.expected_exit_code = arg.parse::<i32>()?;
        }
        "Ignored" => config.ignored = Some(arg.to_owned()),
        "SkipEngine" => {
            let (engine, reason) = arg
                .split_once(':')
                .ok_or_else(|| anyhow!("missing colon separator for SkipEngine"))?;
            if let Some(engine) = match engine.to_lowercase().as_str() {
                "llvm" => Some(Engine::LLVM),
                "cranelift" => Some(Engine::Cranelift),
                "v8" => None,
                _ => bail!("unsupported engine: '{engine}'"),
            } {
                config.skipped_engines.push((engine, reason.to_owned()));
            }
        }
        "UnixOnly" => config.unix_only = arg.parse::<bool>()?,
        "MappedDirectory" => {
            let (host, guest) = arg
                .split_once(':')
                .ok_or_else(|| anyhow!("missing colon separator for MappedDirectory"))?;
            config.mapped_directories.push(MappedDirectory {
                host: HostMappedLocation::new(host),
                guest: guest.to_owned(),
            });
        }
        "CurrentDirectory" => {
            config.current_directory = Some(arg.to_owned());
        }
        "PrefilledFile" => {
            let (path, file_content) = arg
                .split_once(':')
                .ok_or_else(|| anyhow!("missing colon separator for PrefilledFile"))?;
            let path = PathBuf::from(path);
            ensure!(
                path.is_relative(),
                "PrefilledFile must be relative: {path:?}"
            );
            config.prefilled_files.push((path, file_content.to_owned()));
        }
        "ExpectedFile" => {
            let (path, file_content) = arg
                .split_once(':')
                .ok_or_else(|| anyhow!("missing colon separator for ExpectedFile"))?;
            let path = PathBuf::from(path);
            ensure!(
                path.is_relative(),
                "ExpectedFile must be relative: {path:?}"
            );
            config.expected_files.push((path, file_content.to_owned()));
        }
        other => bail!("Unknown directive '{other}'"),
    }
    Ok(())
}

fn run_build_script(config: &Config) -> anyhow::Result<PathBuf> {
    // First, copy the test source directory to the 'build' subfolder that will
    // be unique for each configuration of a test.
    let build_test_path = config.build_path();
    if build_test_path.exists() {
        remove_dir_all(&build_test_path)?;
    }
    create_dir_all(&build_test_path)?;

    let mut options = fs_extra::dir::CopyOptions::new();
    options.content_only = true;
    fs_extra::dir::copy(&config.test_src_dir, &build_test_path, &options).with_context(|| {
        anyhow!(
            "cannot copy {:?} to the temporary directory: {:?}",
            config.test_src_dir,
            &build_test_path,
        )
    })?;

    let mut cmd = match &config.source {
        PrimarySource::BashScript(filename) => {
            let mut cmd = Command::new("bash");
            cmd.arg(build_test_path.join(filename))
                .current_dir(&build_test_path)
                .env("CC", "wasixcc")
                .env("CXX", "wasix++")
                .env("WASIXCC_DISCARD_UNSUPPORTED_FLAGS", "yes");
            cmd
        }
        PrimarySource::SourceFile(filename) => {
            // No build.sh — find a compilable source file and invoke the compiler directly.
            // Priority: main.c > main.cpp > any single .c > any single .cpp
            let primary_source = build_test_path.join(filename);
            let compiler = match primary_source
                .extension()
                .expect("valid extension expected")
                .to_str()
                .expect("string expected")
            {
                "c" => std::env::var("CC").unwrap_or_else(|_| "wasixcc".to_string()),
                "cpp" => std::env::var("CXX").unwrap_or_else(|_| "wasix++".to_string()),
                _ => unreachable!("missing primary source file: {build_test_path:?}"),
            };
            let mut cmd = Command::new(&compiler);
            cmd.arg(&primary_source)
                .arg("-o")
                .arg("main")
                .current_dir(&build_test_path)
                .env("WASIXCC_DISCARD_UNSUPPORTED_FLAGS", "yes");
            cmd
        }
    };

    for (k, v) in &config.build_env {
        cmd.env(k, v);
    }
    let output = cmd.output()?;

    if !output.status.success() {
        eprintln!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("Build failed for {}", build_test_path.display());
    }

    Ok(build_test_path.join("main"))
}

fn run_integration_test(config: Config) -> Result<libtest_mimic::Completion> {
    if let Some(reason) = &config.ignored {
        return Ok(libtest_mimic::Completion::ignored_with(reason.clone()));
    }
    if !cfg!(unix) && config.unix_only {
        return Ok(libtest_mimic::Completion::ignored_with("Unix only"));
    }
    if let Some((_, reason)) = config
        .skipped_engines
        .iter()
        .find(|(engine, _)| *engine == config.engine)
    {
        return Ok(libtest_mimic::Completion::ignored_with(reason.clone()));
    }

    let wasm = run_build_script(&config)?;
    let run_dir = &config.build_path();
    for (path, file_content) in &config.prefilled_files {
        File::create(run_dir.join(path))?.write_all(file_content.as_bytes())?;
    }

    let mut extra_temporary_folders = Vec::new();
    let result = runner::run_wasm_with_runner_config(&wasm, run_dir, config.engine, |runner| {
        if !config.arguments.is_empty() {
            runner.with_args(config.arguments.iter().cloned());
        }

        let mapped_directories = config.mapped_directories.iter().map(|directory| {
            let host = match &directory.host {
                HostMappedLocation::HostPath(host) => {
                    let host = PathBuf::from(host);
                    if host.is_absolute() {
                        host
                    } else {
                        config.build_path().join(host)
                    }
                }
                HostMappedLocation::TemporaryFolder => {
                    let temp = tempfile::tempdir().expect("temporary directory must exist");
                    let host = temp.path().to_path_buf();
                    extra_temporary_folders.push(temp);
                    host
                }
            };

            wasmer_wasix::runners::MappedDirectory {
                host,
                guest: directory.guest.clone(),
            }
        });
        runner.with_mapped_directories(mapped_directories);
        if let Some(current_directory) = &config.current_directory {
            runner.with_current_dir(current_directory.clone());
        }
    })?;

    if config.nonzero_exit_code {
        ensure!(
            result.exit_code != 0,
            "{} expected non-zero exit code\n{}",
            config.test_name,
            runner::format_captured_output(&result),
        );
    } else if result.exit_code != config.expected_exit_code {
        bail!(
            "{} expected exit code {}, got {:?}\n{}",
            config.test_name,
            config.expected_exit_code,
            result.exit_code,
            runner::format_captured_output(&result),
        );
    }

    if !config.expected_stdout.is_empty() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let result_lines: Vec<_> = stdout.trim().lines().collect();
        if result_lines != config.expected_stdout {
            bail!(
                "{} expected stdout `{:?}`, got `{:?}`\n{}",
                config.test_name,
                config.expected_stdout,
                result_lines,
                runner::format_captured_output(&result),
            )
        }
    }

    for (path, expected_content) in config.expected_files {
        let content = std::fs::read_to_string(run_dir.join(&path))
            .with_context(|| format!("{} failed to read {}", config.test_name, path.display()))?;
        ensure!(
            content == expected_content,
            "{} expected file {} to contain `{:?}`, got `{:?}`\n{}",
            config.test_name,
            path.display(),
            expected_content,
            content,
            runner::format_captured_output(&result),
        );
    }

    Ok(libtest_mimic::Completion::Completed)
}

const PRIMARY_SOURCE_FILES: &[&str] = &["main.c", "main.cpp", "build.sh"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrimarySource {
    SourceFile(String),
    BashScript(String),
}

impl PrimarySource {
    fn config_name(&self) -> String {
        match self {
            Self::SourceFile(_) => "default".to_owned(),
            Self::BashScript(path) => {
                if path == "build.sh" {
                    "default".to_owned()
                } else {
                    path.split_once(".")
                        .expect(".sh extension expected")
                        .0
                        .to_string()
                }
            }
        }
    }

    fn filename(&self) -> String {
        match self {
            Self::SourceFile(filename) => filename.clone(),
            Self::BashScript(filename) => filename.clone(),
        }
    }

    fn is_default(&self) -> bool {
        match self {
            Self::SourceFile(_) => true,
            Self::BashScript(filename) => filename == "build.sh",
        }
    }
}

fn identify_primary_sources(test_src_dir: &Path) -> Result<Vec<PrimarySource>> {
    let shell_sources = read_dir(test_src_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("sh"))
        .map(|path| {
            PrimarySource::BashScript(
                path.file_name()
                    .expect("valid filename")
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect_vec();
    if !shell_sources.is_empty() {
        return Ok(shell_sources);
    }

    for file in ["main.c", "main.cpp"] {
        let path = test_src_dir.join(file);
        if path.exists() {
            return Ok(vec![PrimarySource::SourceFile(file.to_string())]);
        }
    }

    bail!(
        "{} must contain {}",
        test_src_dir.display(),
        PRIMARY_SOURCE_FILES.join(",")
    );
}

fn collect_tests(tests: &mut Vec<Trial>) -> Result<()> {
    // Windows runtime support is still limited, so skip these tests on that platform.
    if cfg!(target_os = "windows") {
        return Ok(());
    }

    let tests_dir = PathBuf::from_str(env!("CARGO_MANIFEST_DIR"))?.join("tests/wasm_tests/");
    let tests_build_root = tests_dir.join("build");

    for entry in WalkDir::new(&tests_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path() != tests_dir)
        .filter(|e| e.path().strip_prefix(&tests_build_root).is_err())
        // Skip temporary helper directories (like 'a', 'b', etc.).
        .filter(|e| e.file_type().is_dir())
        .filter(|e| {
            std::fs::read_dir(e.path())
                .expect("valid directory entry")
                .filter_map(Result::ok)
                .any(|entry| {
                    PRIMARY_SOURCE_FILES
                        .contains(&entry.file_name().to_str().expect("filename must be valid"))
                })
        })
    {
        let relative_test_path = entry.path().strip_prefix(&tests_dir)?;

        let test_name = relative_test_path.display().to_string();
        let primary_sources = identify_primary_sources(entry.path())?;

        for primary_source in primary_sources {
            let configs = parse_configs(&Config::new(
                primary_source,
                entry.path().to_path_buf(),
                tests_build_root.clone(),
                test_name.clone(),
            ))?;

            for config in configs {
                for engine in [Engine::Cranelift, Engine::LLVM] {
                    // The EH support for macOS is still missing: #6419
                    if cfg!(target_os = "macos") {
                        return Ok(());
                    }

                    let mut config = config.clone();
                    config.engine = engine;
                    tests.push(libtest_mimic::Trial::ignorable_test(
                        config.full_test_name(),
                        move || {
                            run_integration_test(config)
                                .map_err(|e| libtest_mimic::Failed::from(e.to_string()))
                        },
                    ));
                }
            }
        }
    }

    Ok(())
}
