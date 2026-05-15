//! Tests that build and run various WASIX test programs.
//!
//! Primary test files can contain directives that configure how each WASM test is built,
//! run, and checked. Directives use `//#Directive: Args` in C/C++ sources and
//! `##Directive: Args` in shell sources.
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
//! `ExpectedStdout:{line}` appends one expected stdout line.
//! Can be used multiple times and all expected lines must match the trimmed stdout exactly.
//!
//! `MustFail:{bool}` requires a non-zero exit code when true.
//!
//! `ExpectedExitCode:{code}` sets the expected numeric exit code.
//!
//! `Tempdir:{bool}` runs the test in a fresh temporary working directory when true.
//!
//! `Ignored:{reason}` marks the configuration as ignored with the given reason.
//!
//! `UnixOnly:{bool}` ignores the configuration on non-Unix hosts when true.
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
use std::collections::HashMap;
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::bail;
use libtest_mimic::Trial;
use walkdir::WalkDir;

#[allow(dead_code, unused_imports)]
#[path = "wasm_tests/mod.rs"]
mod wasm_test_helpers;

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    /// The directory containing the test sources.
    test_src_dir: PathBuf,

    test_name: String,
    config_name: String,
    is_abstract: bool,

    nonzero_exit_code: bool,
    expected_exit_code: i32,
    expected_stdout: Vec<String>,
    arguments: Vec<String>,
    tempdir_as_workdir: bool,
    ignored: Option<String>,
    unix_only: bool,
    mapped_directories: Vec<MappedDirectory>,
    current_directory: Option<String>,
    prefilled_files: Vec<(PathBuf, String)>,
    expected_files: Vec<(PathBuf, String)>,
}

impl Config {
    fn new(test_src_dir: PathBuf, test_name: String) -> Self {
        Self {
            test_src_dir,
            test_name,
            config_name: "default".to_owned(),
            is_abstract: false,
            arguments: Vec::new(),
            nonzero_exit_code: false,
            expected_exit_code: 0,
            expected_stdout: Vec::new(),
            tempdir_as_workdir: false,
            ignored: None,
            unix_only: false,
            mapped_directories: Vec::new(),
            current_directory: None,
            prefilled_files: Vec::new(),
            expected_files: Vec::new(),
        }
    }
}

fn parse_configs(src_filename: &Path, default_config: &Config) -> Result<Vec<Config>> {
    let source = std::fs::read_to_string(src_filename)
        .with_context(|| format!("Failed to read {}", src_filename.display()))?;

    let mut configs = Vec::new();
    let mut config_name_to_index = HashMap::new();
    let mut config = default_config.clone();

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

    configs.retain(|c| !c.is_abstract);

    if configs.is_empty() {
        bail!("Missing non-abstract Config");
    }

    Ok(configs)
}

fn process_directive(
    rest: &str,
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
        "MustFail" => {
            config.nonzero_exit_code = arg.parse::<bool>()?;
        }
        "ExpectedExitCode" => {
            config.expected_exit_code = arg.parse::<i32>()?;
        }
        "Tempdir" => {
            config.tempdir_as_workdir = arg.parse::<bool>()?;
        }
        "Ignored" => config.ignored = Some(arg.to_owned()),
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
                "PrefilledPath must be relative: {path:?}"
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

fn run_integration_test(config: Config) -> Result<libtest_mimic::Completion> {
    if let Some(reason) = config.ignored {
        return Ok(libtest_mimic::Completion::ignored_with(reason));
    }
    if !cfg!(unix) && config.unix_only {
        return Ok(libtest_mimic::Completion::ignored_with("Unix only"));
    }

    // TODO: remove
    let wasm = wasm_test_helpers::run_build_script("x.rs", config.test_src_dir.to_str().unwrap())?;
    let temp_dir = config
        .tempdir_as_workdir
        .then(|| tempfile::tempdir().expect("temporary directory must exist"));
    let run_dir = if let Some(temp_dir) = &temp_dir {
        temp_dir.path()
    } else {
        wasm.parent()
            .with_context(|| format!("{} has no parent directory", wasm.display()))?
    };
    for (path, file_content) in config.prefilled_files {
        File::create(run_dir.join(path))?.write_all(file_content.as_bytes())?;
    }

    let mut extra_temporary_folders = Vec::new();
    let result = wasm_test_helpers::run_wasm_with_runner_config(&wasm, run_dir, |runner| {
        if !config.arguments.is_empty() {
            runner.with_args(config.arguments);
        }

        let mapped_directories = config.mapped_directories.into_iter().map(|directory| {
            let host = match directory.host {
                HostMappedLocation::HostPath(host) => {
                    let host = PathBuf::from(host);
                    if host.is_absolute() {
                        host
                    } else {
                        config.test_src_dir.join(host)
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
                guest: directory.guest,
            }
        });
        runner.with_mapped_directories(mapped_directories);
        if let Some(current_directory) = config.current_directory {
            runner.with_current_dir(current_directory);
        }
    })?;

    if config.nonzero_exit_code {
        ensure!(
            result.exit_code.is_some_and(|exit_code| exit_code != 0),
            "{} expected non-zero exit code\n{}",
            config.test_name,
            format_captured_output(&result),
        );
    } else if result.exit_code.is_none() && config.expected_exit_code == 0 {
        // OK
    } else if result.exit_code != Some(config.expected_exit_code) {
        bail!(
            "{} expected exit code {}, got {:?}\n{}",
            config.test_name,
            config.expected_exit_code,
            result.exit_code,
            format_captured_output(&result),
        );
    }

    if !config.expected_stdout.is_empty() {
        // TODO: improve
        let stdout = String::from_utf8_lossy(&result.stdout);
        let result_lines: Vec<_> = stdout.trim().lines().collect();
        if result_lines != config.expected_stdout {
            bail!(
                "{} expected stdout `{:?}`, got `{:?}`\n",
                config.test_name,
                config.expected_stdout,
                result_lines
            )
        }
    }

    for (path, expected_content) in config.expected_files {
        let content = std::fs::read_to_string(run_dir.join(&path))
            .with_context(|| format!("{} failed to read {}", config.test_name, path.display()))?;
        ensure!(
            content == expected_content,
            "{} expected file {} to contain `{:?}`, got `{:?}`",
            config.test_name,
            path.display(),
            expected_content,
            content
        );
    }

    Ok(libtest_mimic::Completion::Completed)
}

fn format_captured_output(result: &wasm_test_helpers::WasmRunResult) -> String {
    let mut message = format!(
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}\ntrace:\n{}",
        result.exit_code,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.trace_output),
    );

    if let Some(error) = &result.error {
        message.push_str(&format!("\nerror:\n{}", error));
    }

    message
}

const PRIMARY_SOURCE_FILES: &[&str] = &["main.c", "main.cpp", "build.sh"];

fn identify_primary_source(test_src_dir: &Path) -> Result<PathBuf> {
    for file in PRIMARY_SOURCE_FILES {
        let path = test_src_dir.join(file);
        if path.exists() {
            return Ok(path);
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

    for entry in WalkDir::new(&tests_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path() != tests_dir)
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
        let test_name = entry.path().strip_prefix(&tests_dir)?.display().to_string();
        let primary_source = identify_primary_source(entry.path())?;

        let configs = parse_configs(
            &primary_source,
            &Config::new(entry.path().to_path_buf(), test_name.clone()),
        )?;

        for config in configs {
            // TODO: strip "wasm" ??
            let full_name = format!("wasm/{}/{}", test_name, config.config_name);

            tests.push(libtest_mimic::Trial::ignorable_test(full_name, move || {
                run_integration_test(config).map_err(|e| libtest_mimic::Failed::from(e.to_string()))
            }));
        }
    }

    Ok(())
}
