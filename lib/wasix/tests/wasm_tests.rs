use anyhow::{Context, Result, anyhow, ensure};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::bail;
use libtest_mimic::Trial;
use walkdir::WalkDir;

#[allow(dead_code, unused_imports)]
#[path = "wasm_tests/mod.rs"]
mod wasm_test_helpers;

struct Filter {
    filter: Option<String>,
    exact: bool,
}

impl Filter {
    fn new(args: &libtest_mimic::Arguments) -> Self {
        Self {
            filter: args.filter.clone(),
            exact: args.exact,
        }
    }

    /// TODO: write comment
    fn excludes(&self, prefix: &str) -> bool {
        let Some(filter) = self.filter.as_ref() else {
            return false;
        };

        if self.exact {
            !filter.starts_with(prefix) && !prefix.starts_with(filter)
        } else {
            false
        }
    }
}

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
    let filter = Filter::new(&args);
    let mut tests = Vec::new();
    collect_tests(&mut tests, &filter)?;
    Ok(libtest_mimic::run(&args, tests).exit_code())
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
    expected_stdout: Option<String>,
    arguments: Vec<String>,
    tempdir_as_workdir: bool,
    ignored: Option<String>,
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
            expected_stdout: None,
            tempdir_as_workdir: false,
            ignored: None,
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
            config.expected_stdout = Some(arg.to_owned());
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
        other => bail!("Unknown directive '{other}'"),
    }
    Ok(())
}

fn run_integration_test(mut config: Config) -> Result<libtest_mimic::Completion> {
    if let Some(reason) = config.ignored {
        return Ok(libtest_mimic::Completion::ignored_with(reason));
    }

    let (module, test_dir) = config
        .test_name
        .split_once('/')
        .with_context(|| format!("invalid test name `{}`", config.test_name))?;
    let module_file = format!("{module}.rs");

    let wasm = wasm_test_helpers::run_build_script(&module_file, test_dir)?;
    let temp_dir = config
        .tempdir_as_workdir
        .then(|| tempfile::tempdir().expect("temporary directory must exist"));
    let run_dir = if let Some(temp_dir) = &temp_dir {
        temp_dir.path()
    } else {
        wasm.parent()
            .with_context(|| format!("{} has no parent directory", wasm.display()))?
    };
    let result = if config.arguments.is_empty() {
        wasm_test_helpers::run_wasm_with_result(&wasm, run_dir)?
    } else {
        wasm_test_helpers::run_wasm_with_runner_config(&wasm, run_dir, |runner| {
            runner.with_args(config.arguments);
        })?
    };

    if config.nonzero_exit_code {
        ensure!(
            result.exit_code.is_some_and(|exit_code| exit_code != 0),
            "{} expected non-zero exit code\n{}",
            config.test_name,
            format_captured_output(&result),
        );
    } else if result.exit_code != Some(config.expected_exit_code) {
        bail!(
            "{} expected exit code {}, got {:?}\n{}",
            config.test_name,
            config.expected_exit_code,
            result.exit_code,
            format_captured_output(&result),
        );
    }

    if let Some(expected_stdout) = config.expected_stdout.take() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        if stdout.trim() != expected_stdout {
            bail!(
                "{} expected stdout `{}`, got `{}`\n{}",
                config.test_name,
                expected_stdout,
                stdout.trim(),
                format_captured_output(&result),
            );
        }
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

fn identify_primary_source(test_src_dir: &Path) -> Result<PathBuf> {
    const FILES: &[&str] = &["main.c", "main.cpp", "build.sh"];

    for file in FILES {
        let path = test_src_dir.join(file);
        if path.exists() {
            return Ok(path);
        }
    }

    bail!(
        "{} must contain {}",
        test_src_dir.display(),
        FILES.join(",")
    );
}

fn collect_tests(tests: &mut Vec<Trial>, filter: &Filter) -> Result<()> {
    let tests_dir = PathBuf::from_str(env!("CARGO_MANIFEST_DIR"))?.join("tests/wasm_tests/");

    for entry in WalkDir::new(&tests_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path() != tests_dir)
        // Skip temporary helper directories (like 'a', 'b', etc.).
        .filter(|e| {
            e.path()
                .file_name()
                .expect("file name must be valid")
                .to_str()
                .expect("valid path")
                .len()
                > 1
        })
        .filter(|e| e.file_type().is_dir())
        // TODO
        .filter(|e| {
            std::fs::read_dir(e.path())
                .expect("valid directory entry")
                .filter_map(Result::ok)
                .any(|entry| entry.file_type().expect("cannot read file type").is_file())
        })
    {
        let test_name = entry.path().strip_prefix(&tests_dir)?.display().to_string();
        let primary_source = identify_primary_source(entry.path())?;

        let configs = parse_configs(
            &primary_source,
            &Config::new(entry.path().to_path_buf(), test_name.clone()),
        )?;

        for config in configs {
            // TODO: strip "wasm"
            let full_name = format!("wasm/{}/{}", test_name, config.config_name);

            tests.push(libtest_mimic::Trial::ignorable_test(full_name, move || {
                run_integration_test(config).map_err(|e| libtest_mimic::Failed::from(e.to_string()))
            }));
        }
    }

    Ok(())
}
