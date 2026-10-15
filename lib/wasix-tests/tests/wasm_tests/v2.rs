//! The WASIX v2 target: building fixtures with `wasixcc` in `wasm32-wasixv2` mode and running
//! them on `wasixv2::Machine`. See the README for which fixtures get a v2 variant.

// Windows has neither Cranelift nor wasixcc, so no v2 variants are registered there.
#![cfg_attr(target_os = "windows", allow(dead_code))]

use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Write as _};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt};
use wasixv2::{ByteSink, ByteSource, LocalFuture, Machine, ProcessConfig, Stdio};

use crate::runner::WasmRunResult;

/// Test names of v2 variants known to fail, relative to the crate root.
pub(crate) const EXPECTED_FAILURES_FILE: &str = "v2-expected-failures.txt";

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const RECENT_SYSCALLS: usize = 40;

/// Where the v2 toolchain lives. Locations have their own variables rather than the `WASIXCC_*`
/// ones, which every v1 build inherits too.
#[derive(Debug, Clone)]
pub(crate) struct Toolchain {
    wasixcc: PathBuf,
    llvm_location: PathBuf,
    sysroot: PathBuf,
}

impl Toolchain {
    /// Finds the toolchain, or says why it is not usable.
    pub(crate) fn detect() -> Result<Self, String> {
        let wasixcc = match std::env::var_os("WASIXCC") {
            Some(path) => PathBuf::from(path),
            None => find_on_path("wasixcc").ok_or("wasixcc is not on PATH (or set WASIXCC)")?,
        };
        let home = dirs::home_dir().map(|home| home.join(".wasixcc"));
        let location = |var: &str, default: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(default)))
                .ok_or_else(|| format!("cannot find the home directory; set {var}"))
        };

        let llvm_location = location("WASIX_TESTS_V2_LLVM_LOCATION", "llvm-wasixv2")?;
        if !llvm_location.join("bin/clang").is_file() {
            return Err(format!(
                "no wasm32-wasixv2 LLVM at {} (set WASIX_TESTS_V2_LLVM_LOCATION)",
                llvm_location.display()
            ));
        }
        let sysroot = location("WASIX_TESTS_V2_SYSROOT", "sysroot-wasixv2")?;
        if !sysroot.join("lib/wasm32-wasixv2/libc.a").is_file() {
            return Err(format!(
                "no wasm32-wasixv2 sysroot at {} (set WASIX_TESTS_V2_SYSROOT)",
                sysroot.display()
            ));
        }

        Ok(Self {
            wasixcc,
            llvm_location,
            sysroot,
        })
    }

    pub(crate) fn wasixcc(&self) -> &Path {
        &self.wasixcc
    }

    /// Selects v2 for `wasixcc`, overriding whatever `WASIXCC_*` locations the environment has.
    pub(crate) fn build_env(&self) -> [(&'static str, &Path); 3] {
        [
            ("WASIXCC_TARGET", Path::new("wasm32-wasixv2")),
            ("WASIXCC_LLVM_LOCATION", &self.llvm_location),
            ("WASIXCC_SYSROOT", &self.sysroot),
        ]
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Whether a shell fixture only builds C, which is all `wasixcc` supports for v2 so far.
pub(crate) fn shell_script_is_c_only(script: &str) -> bool {
    const NOT_C: &[&str] = &["CXX", "wasix++", ".cpp", "cargo", "rustc"];
    !NOT_C.iter().any(|marker| script.contains(marker))
}

/// Reads the expected-failure list: one test name per line, `#` starts a comment.
pub(crate) fn load_expected_failures(path: &Path) -> Result<HashSet<String>> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut names = HashSet::new();
    for (i, line) in contents.lines().enumerate() {
        let name = line.split('#').next().unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        ensure!(
            names.insert(name.to_owned()),
            "{}:{}: `{name}` is listed twice",
            path.display(),
            i + 1
        );
    }
    Ok(names)
}

pub(crate) fn ensure_wasixv2_module(wasm: &[u8]) -> Result<()> {
    ensure!(
        wasixv2::is_wasixv2_module(wasm),
        "the build did not produce a wasixv2 module; does this wasixcc support \
         WASIXCC_TARGET=wasm32-wasixv2?"
    );
    Ok(())
}

pub(crate) struct Process {
    pub(crate) wasm: Vec<u8>,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<String>,
    pub(crate) stdin: Vec<u8>,
}

#[derive(Clone, Default)]
struct Captured {
    stdout: Arc<Capture>,
    stderr: Arc<Capture>,
    syscalls: Arc<Mutex<SyscallLog>>,
}

impl Captured {
    fn result(&self, exit_code: i32, error: Option<String>) -> WasmRunResult {
        WasmRunResult {
            stdout: self.stdout.contents(),
            stderr: self.stderr.contents(),
            trace_output: self.syscalls.lock().unwrap().to_string().into_bytes(),
            exit_code,
            error,
        }
    }
}

/// Runs `process` on a fresh machine. The machine is not `Send`, so it gets a thread of its own,
/// which is abandoned if the guest outlives the timeout (`WASIX_TESTS_V2_TIMEOUT_SECS`).
pub(crate) fn run(process: Process) -> Result<WasmRunResult> {
    let timeout = std::env::var("WASIX_TESTS_V2_TIMEOUT_SECS")
        .ok()
        .and_then(|secs| secs.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let captured = Captured::default();
    let (done, finished) = mpsc::channel();
    std::thread::Builder::new()
        .name("wasixv2-guest".to_owned())
        .spawn({
            let captured = captured.clone();
            move || {
                let _ = done.send(run_on_this_thread(process, &captured));
            }
        })
        .context("failed to spawn the guest thread")?;

    match finished.recv_timeout(Duration::from_secs(timeout)) {
        Ok(Ok(status)) => Ok(captured.result(status, None)),
        // Like v1, a guest that does not exit (a trap, a link error, ...) fails the test.
        Ok(Err(error)) => bail!(
            "{}",
            crate::runner::format_captured_output(&captured.result(-1, Some(format!("{error:#}"))))
        ),
        Err(mpsc::RecvTimeoutError::Timeout) => bail!(
            "timed out after {timeout}s\n{}",
            crate::runner::format_captured_output(&captured.result(-1, None))
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => bail!("the guest thread panicked"),
    }
}

fn run_on_this_thread(process: Process, captured: &Captured) -> Result<i32> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create the tokio runtime")?;
    let config = ProcessConfig {
        args: process.args,
        env: process.env,
        stdio: Stdio {
            stdin: Arc::new(Input(Mutex::new(io::Cursor::new(process.stdin)))),
            stdout: captured.stdout.clone(),
            stderr: captured.stderr.clone(),
        },
    };
    let subscriber =
        tracing_subscriber::registry().with(SyscallRecorder(captured.syscalls.clone()));
    let status = tracing::subscriber::with_default(subscriber, || {
        runtime
            .block_on(Machine::new().run_process(&process.wasm, config))
            .map_err(anyhow::Error::from)
    })?;
    Ok(status)
}

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl Capture {
    fn contents(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

struct Input(Mutex<io::Cursor<Vec<u8>>>);

impl ByteSource for Input {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        let read = self.0.lock().unwrap().read(buf);
        Box::pin(async move { read })
    }
}

/// What the kernel's trace layer reported: which syscalls answered `ENOSYS`, and the last few
/// calls, which is usually enough to see why a guest failed.
#[derive(Default)]
struct SyscallLog {
    enosys: BTreeMap<String, usize>,
    recent: Vec<String>,
}

impl fmt::Display for SyscallLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let enosys = self
            .enosys
            .iter()
            .map(|(name, count)| format!("{name} x{count}"))
            .collect::<Vec<_>>();
        writeln!(f, "enosys: {}", enosys.join(", "))?;
        writeln!(f, "last {} syscalls:", self.recent.len())?;
        for line in &self.recent {
            writeln!(f, "  {line}")?;
        }
        Ok(())
    }
}

struct SyscallRecorder(Arc<Mutex<SyscallLog>>);

impl<S: tracing::Subscriber> Layer<S> for SyscallRecorder {
    fn on_event(&self, event: &tracing::Event<'_>, _: LayerContext<'_, S>) {
        if event.metadata().target() != "wasixv2_trace" {
            return;
        }
        let mut fields = SyscallFields::default();
        event.record(&mut fields);
        let mut log = self.0.lock().unwrap();
        if fields.ret.contains("ENOSYS") {
            let name = fields
                .call
                .split([' ', '{', '('])
                .next()
                .unwrap_or_default();
            *log.enosys.entry(name.to_owned()).or_default() += 1;
        }
        if log.recent.len() == RECENT_SYSCALLS {
            log.recent.remove(0);
        }
        log.recent
            .push(format!("{} -> {}", fields.call, fields.ret));
    }
}

#[derive(Default)]
struct SyscallFields {
    call: String,
    ret: String,
}

impl Visit for SyscallFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let target = match field.name() {
            "call" => &mut self.call,
            "ret" => &mut self.ret,
            _ => return,
        };
        let _ = write!(target, "{value:?}");
    }
}
