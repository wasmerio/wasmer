//! Builder system for configuring a [`WasiState`] and creating it.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use rand::Rng;
use thiserror::Error;
use virtual_fs::{ArcFile, FileSystem, FsError, TmpFileSystem, VirtualFile};
use wasmer::{AsStoreMut, Engine, Instance, Module};
use wasmer_config::package::PackageId;

#[cfg(feature = "journal")]
use crate::journal::{DynJournal, DynReadableJournal, SnapshotTrigger};
use crate::{
    Runtime, WasiEnv, WasiFunctionEnv, WasiRuntimeError, WasiThreadError,
    bin_factory::{BinFactory, BinaryPackage},
    capabilities::Capabilities,
    fs::{WasiFs, WasiFsRoot, WasiInodes},
    os::task::control_plane::{ControlPlaneConfig, ControlPlaneError, WasiControlPlane},
    state::WasiState,
    syscalls::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO},
};
use wasmer_types::ModuleHash;
use wasmer_wasix_types::wasi::SignalDisposition;

use super::env::WasiEnvInit;

// FIXME: additional import support was broken and has been removed. We need to re-introduce
// it in a way that works with multi-threaded WASIX apps.
/// Builder API for configuring a [`WasiEnv`] environment needed to run WASI modules.
///
/// Usage:
/// ```no_run
/// # use wasmer_wasix::{WasiEnv, WasiStateCreationError};
/// # fn main() -> Result<(), WasiStateCreationError> {
/// let mut state_builder = WasiEnv::builder("wasi-prog-name");
/// state_builder
///    .env("ENV_VAR", "ENV_VAL")
///    .arg("--verbose")
///    .preopen_dir("src")?
///    .map_dir("name_wasi_sees", "path/on/host/fs")?
///    .build_init()?;
/// # Ok(())
/// # }
/// ```
#[derive(Default)]
pub struct WasiEnvBuilder {
    /// Name of entry function. Defaults to running `_start` if not specified.
    pub(super) entry_function: Option<String>,
    /// Command line arguments.
    pub(super) args: Vec<String>,
    /// Environment variables.
    pub(super) envs: Vec<(String, Vec<u8>)>,
    /// Signals that should get their handler overridden.
    pub(super) signals: Vec<SignalDisposition>,
    /// Pre-opened directories that will be accessible from WASI.
    pub(super) preopens: Vec<PreopenedDir>,
    /// Pre-opened virtual directories that will be accessible from WASI.
    vfs_preopens: Vec<String>,
    #[allow(clippy::type_complexity)]
    pub(super) setup_fs_fn:
        Option<Box<dyn Fn(&WasiInodes, &mut WasiFs) -> Result<(), String> + Send>>,
    pub(super) stdout: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
    pub(super) stderr: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
    pub(super) stdin: Option<Box<dyn VirtualFile + Send + Sync + 'static>>,
    pub(super) fs: Option<WasiFsRoot>,
    pub(super) engine: Option<Engine>,
    pub(super) runtime: Option<Arc<dyn crate::Runtime + Send + Sync + 'static>>,
    pub(super) current_dir: Option<PathBuf>,

    /// List of webc dependencies to be injected.
    pub(super) uses: Vec<BinaryPackage>,

    pub(super) included_packages: HashSet<PackageId>,

    pub(super) module_hash: Option<ModuleHash>,

    /// List of host commands to map into the WASI instance.
    pub(super) map_commands: HashMap<String, PathBuf>,

    pub(super) capabilites: Capabilities,

    #[cfg(feature = "journal")]
    pub(super) snapshot_on: Vec<SnapshotTrigger>,

    #[cfg(feature = "journal")]
    pub(super) snapshot_interval: Option<std::time::Duration>,

    #[cfg(feature = "journal")]
    pub(super) stop_running_after_snapshot: bool,

    #[cfg(feature = "journal")]
    pub(super) read_only_journals: Vec<Arc<DynReadableJournal>>,

    #[cfg(feature = "journal")]
    pub(super) writable_journals: Vec<Arc<DynJournal>>,

    pub(super) skip_stdio_during_bootstrap: bool,

    #[cfg(feature = "ctrlc")]
    pub(super) attach_ctrl_c: bool,
}

impl std::fmt::Debug for WasiEnvBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO: update this when stable
        f.debug_struct("WasiEnvBuilder")
            .field("entry_function", &self.entry_function)
            .field("args", &self.args)
            .field("envs", &self.envs)
            .field("signals", &self.signals)
            .field("preopens", &self.preopens)
            .field("uses", &self.uses)
            .field("setup_fs_fn exists", &self.setup_fs_fn.is_some())
            .field("stdout_override exists", &self.stdout.is_some())
            .field("stderr_override exists", &self.stderr.is_some())
            .field("stdin_override exists", &self.stdin.is_some())
            .field("engine_override_exists", &self.engine.is_some())
            .field("runtime_override_exists", &self.runtime.is_some())
            .finish()
    }
}

/// Error type returned when bad data is given to [`WasiEnvBuilder`].
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WasiStateCreationError {
    #[error("bad environment variable format: `{0}`")]
    EnvironmentVariableFormatError(String),
    #[error("argument contains null byte: `{0}`")]
    ArgumentContainsNulByte(String),
    #[error("preopened directory not found: `{0}`")]
    PreopenedDirectoryNotFound(PathBuf),
    #[error("preopened directory error: `{0}`")]
    PreopenedDirectoryError(String),
    #[error("mapped dir alias has wrong format: `{0}`")]
    MappedDirAliasFormattingError(String),
    #[error("wasi filesystem creation error: `{0}`")]
    WasiFsCreationError(String),
    #[error("wasi filesystem setup error: `{0}`")]
    WasiFsSetupError(String),
    #[error(transparent)]
    FileSystemError(#[from] FsError),
    #[error("wasi inherit error: `{0}`")]
    WasiInheritError(String),
    #[error("wasi include package: `{0}`")]
    WasiIncludePackageError(String),
    #[error("control plane error")]
    ControlPlane(#[from] ControlPlaneError),
}

fn validate_mapped_dir_alias(alias: &str) -> Result<(), WasiStateCreationError> {
    if !alias.bytes().all(|b| b != b'\0') {
        return Err(WasiStateCreationError::MappedDirAliasFormattingError(
            format!("Alias \"{alias}\" contains a nul byte"),
        ));
    }

    Ok(())
}

pub type SetupFsFn = Box<dyn Fn(&WasiInodes, &mut WasiFs) -> Result<(), String> + Send>;

// TODO add other WasiFS APIs here like swapping out stdout, for example (though we need to
// return stdout somehow, it's unclear what that API should look like)
impl WasiEnvBuilder {
    /// Creates an empty [`WasiEnvBuilder`].
    pub fn new(program_name: impl Into<String>) -> Self {
        WasiEnvBuilder {
            args: vec![program_name.into()],
            ..WasiEnvBuilder::default()
        }
    }

    /// Attaches a ctrl-c handler which will send signals to the
    /// process rather than immediately termiante it
    #[cfg(feature = "ctrlc")]
    pub fn attach_ctrl_c(mut self) -> Self {
        self.attach_ctrl_c = true;
        self
    }

    /// Add an environment variable pair.
    ///
    /// Both the key and value of an environment variable must not
    /// contain a nul byte (`0x0`), and the key must not contain the
    /// `=` byte (`0x3d`).
    pub fn env<Key, Value>(mut self, key: Key, value: Value) -> Self
    where
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        self.add_env(key, value);
        self
    }

    /// Add an environment variable pair.
    ///
    /// Both the key and value of an environment variable must not
    /// contain a nul byte (`0x0`), and the key must not contain the
    /// `=` byte (`0x3d`).
    pub fn add_env<Key, Value>(&mut self, key: Key, value: Value)
    where
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        self.envs.push((
            String::from_utf8_lossy(key.as_ref()).to_string(),
            value.as_ref().to_vec(),
        ));
    }

    /// Add multiple environment variable pairs.
    ///
    /// Both the key and value of the environment variables must not
    /// contain a nul byte (`0x0`), and the key must not contain the
    /// `=` byte (`0x3d`).
    pub fn envs<I, Key, Value>(mut self, env_pairs: I) -> Self
    where
        I: IntoIterator<Item = (Key, Value)>,
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        self.add_envs(env_pairs);

        self
    }

    /// Add multiple environment variable pairs.
    ///
    /// Both the key and value of the environment variables must not
    /// contain a nul byte (`0x0`), and the key must not contain the
    /// `=` byte (`0x3d`).
    pub fn add_envs<I, Key, Value>(&mut self, env_pairs: I)
    where
        I: IntoIterator<Item = (Key, Value)>,
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        for (key, value) in env_pairs {
            self.add_env(key, value);
        }
    }

    /// Get a reference to the configured environment variables.
    pub fn get_env(&self) -> &[(String, Vec<u8>)] {
        &self.envs
    }

    /// Get a mutable reference to the configured environment variables.
    pub fn get_env_mut(&mut self) -> &mut Vec<(String, Vec<u8>)> {
        &mut self.envs
    }

    /// Add a signal handler override.
    pub fn signal(mut self, sig_action: SignalDisposition) -> Self {
        self.add_signal(sig_action);
        self
    }

    /// Add a signal handler override.
    pub fn add_signal(&mut self, sig_action: SignalDisposition) {
        self.signals.push(sig_action);
    }

    /// Add multiple signal handler overrides.
    pub fn signals<I>(mut self, signal_pairs: I) -> Self
    where
        I: IntoIterator<Item = SignalDisposition>,
    {
        self.add_signals(signal_pairs);

        self
    }

    /// Add multiple signal handler overrides.
    pub fn add_signals<I>(&mut self, signal_pairs: I)
    where
        I: IntoIterator<Item = SignalDisposition>,
    {
        for sig in signal_pairs {
            self.add_signal(sig);
        }
    }

    /// Get a reference to the configured signal handler overrides.
    pub fn get_signals(&self) -> &[SignalDisposition] {
        &self.signals
    }

    /// Get a mutable reference to the configured signalironment variables.
    pub fn get_signals_mut(&mut self) -> &mut Vec<SignalDisposition> {
        &mut self.signals
    }

    pub fn entry_function<S>(mut self, entry_function: S) -> Self
    where
        S: AsRef<str>,
    {
        self.set_entry_function(entry_function);
        self
    }

    pub fn set_entry_function<S>(&mut self, entry_function: S)
    where
        S: AsRef<str>,
    {
        self.entry_function = Some(entry_function.as_ref().to_owned());
    }

    /// Add an argument.
    ///
    /// Arguments must not contain the nul (0x0) byte
    // TODO: should take Into<Vec<u8>>
    pub fn arg<V>(mut self, arg: V) -> Self
    where
        V: AsRef<[u8]>,
    {
        self.add_arg(arg);
        self
    }

    /// Add an argument.
    ///
    /// Arguments must not contain the nul (0x0) byte.
    // TODO: should take Into<Vec<u8>>
    pub fn add_arg<V>(&mut self, arg: V)
    where
        V: AsRef<[u8]>,
    {
        self.args
            .push(String::from_utf8_lossy(arg.as_ref()).to_string());
    }

    /// Add multiple arguments.
    ///
    /// Arguments must not contain the nul (0x0) byte
    pub fn args<I, Arg>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = Arg>,
        Arg: AsRef<[u8]>,
    {
        self.add_args(args);

        self
    }

    /// Add multiple arguments.
    ///
    /// Arguments must not contain the nul (0x0) byte
    pub fn add_args<I, Arg>(&mut self, args: I)
    where
        I: IntoIterator<Item = Arg>,
        Arg: AsRef<[u8]>,
    {
        for arg in args {
            self.add_arg(arg);
        }
    }

    /// Get a reference to the configured arguments.
    pub fn get_args(&self) -> &[String] {
        &self.args
    }

    /// Get a mutable reference to the configured arguments.
    pub fn get_args_mut(&mut self) -> &mut Vec<String> {
        &mut self.args
    }

    /// Adds a container this module inherits from.
    ///
    /// This will make all of the container's files and commands available to the
    /// resulting WASI instance.
    pub fn use_webc(mut self, pkg: BinaryPackage) -> Self {
        self.add_webc(pkg);
        self
    }

    /// Sets the module hash for the running process. This ensures that the journal
    /// can restore the records for the right module. If no module hash is supplied
    /// then the process will start with a random module hash.
    pub fn set_module_hash(&mut self, hash: ModuleHash) -> &mut Self {
        self.module_hash.replace(hash);
        self
    }

    /// Adds a container this module inherits from.
    ///
    /// This will make all of the container's files and commands available to the
    /// resulting WASI instance.
    pub fn add_webc(&mut self, pkg: BinaryPackage) -> &mut Self {
        self.uses.push(pkg);
        self
    }

    /// Adds a package that is already included in the [`WasiEnvBuilder`] filesystem.
    /// These packages will not be merged to the final filesystem since they are already included.
    pub fn include_package(&mut self, pkg_id: PackageId) -> &mut Self {
        self.included_packages.insert(pkg_id);
        self
    }

    /// Adds packages that is already included in the [`WasiEnvBuilder`] filesystem.
    /// These packages will not be merged to the final filesystem since they are already included.
    pub fn include_packages(&mut self, pkg_ids: impl IntoIterator<Item = PackageId>) -> &mut Self {
        self.included_packages.extend(pkg_ids);

        self
    }

    /// Adds a list of other containers this module inherits from.
    ///
    /// This will make all of the container's files and commands available to the
    /// resulting WASI instance.
    pub fn uses<I>(mut self, uses: I) -> Self
    where
        I: IntoIterator<Item = BinaryPackage>,
    {
        for pkg in uses {
            self.add_webc(pkg);
        }
        self
    }

    /// Map an atom to a local binary
    pub fn map_command<Name, Target>(mut self, name: Name, target: Target) -> Self
    where
        Name: AsRef<str>,
        Target: AsRef<str>,
    {
        self.add_mapped_command(name, target);
        self
    }

    /// Map an atom to a local binary
    pub fn add_mapped_command<Name, Target>(&mut self, name: Name, target: Target)
    where
        Name: AsRef<str>,
        Target: AsRef<str>,
    {
        let path_buf = PathBuf::from(target.as_ref().to_string());
        self.map_commands
            .insert(name.as_ref().to_string(), path_buf);
    }

    /// Maps a series of atoms to the local binaries
    pub fn map_commands<I, Name, Target>(mut self, map_commands: I) -> Self
    where
        I: IntoIterator<Item = (Name, Target)>,
        Name: AsRef<str>,
        Target: AsRef<str>,
    {
        self.add_mapped_commands(map_commands);
        self
    }

    /// Maps a series of atoms to local binaries.
    pub fn add_mapped_commands<I, Name, Target>(&mut self, map_commands: I)
    where
        I: IntoIterator<Item = (Name, Target)>,
        Name: AsRef<str>,
        Target: AsRef<str>,
    {
        for (alias, target) in map_commands {
            self.add_mapped_command(alias, target);
        }
    }

    /// Preopen a directory
    ///
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn preopen_dir<P>(mut self, po_dir: P) -> Result<Self, WasiStateCreationError>
    where
        P: AsRef<Path>,
    {
        self.add_preopen_dir(po_dir)?;
        Ok(self)
    }

    /// Adds a preopen a directory
    ///
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn add_preopen_dir<P>(&mut self, po_dir: P) -> Result<(), WasiStateCreationError>
    where
        P: AsRef<Path>,
    {
        let mut pdb = PreopenDirBuilder::new();
        let path = po_dir.as_ref();
        pdb.directory(path).read(true).write(true).create(true);
        let preopen = pdb.build()?;

        self.preopens.push(preopen);

        Ok(())
    }

    /// Preopen multiple directories.
    ///
    /// This opens the given directories at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn preopen_dirs<I, P>(mut self, dirs: I) -> Result<Self, WasiStateCreationError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for po_dir in dirs {
            self.add_preopen_dir(po_dir)?;
        }

        Ok(self)
    }

    /// Preopen a directory and configure it.
    ///
    /// Usage:
    ///
    /// ```no_run
    /// # use wasmer_wasix::{WasiEnv, WasiStateCreationError};
    /// # fn main() -> Result<(), WasiStateCreationError> {
    /// WasiEnv::builder("program_name")
    ///    .preopen_build(|p| p.directory("src").read(true).write(true).create(true))?
    ///    .preopen_build(|p| p.directory(".").alias("dot").read(true))?
    ///    .build_init()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn preopen_build<F>(mut self, inner: F) -> Result<Self, WasiStateCreationError>
    where
        F: Fn(&mut PreopenDirBuilder) -> &mut PreopenDirBuilder,
    {
        self.add_preopen_build(inner)?;
        Ok(self)
    }

    /// Preopen a directory and configure it.
    ///
    /// Usage:
    ///
    /// ```no_run
    /// # use wasmer_wasix::{WasiEnv, WasiStateCreationError};
    /// # fn main() -> Result<(), WasiStateCreationError> {
    /// WasiEnv::builder("program_name")
    ///    .preopen_build(|p| p.directory("src").read(true).write(true).create(true))?
    ///    .preopen_build(|p| p.directory(".").alias("dot").read(true))?
    ///    .build_init()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_preopen_build<F>(&mut self, inner: F) -> Result<(), WasiStateCreationError>
    where
        F: Fn(&mut PreopenDirBuilder) -> &mut PreopenDirBuilder,
    {
        let mut pdb = PreopenDirBuilder::new();
        let po_dir = inner(&mut pdb).build()?;

        self.preopens.push(po_dir);

        Ok(())
    }

    /// Preopen the given directories from the
    /// Virtual FS.
    pub fn preopen_vfs_dirs<I>(&mut self, po_dirs: I) -> Result<&mut Self, WasiStateCreationError>
    where
        I: IntoIterator<Item = String>,
    {
        for po_dir in po_dirs {
            self.vfs_preopens.push(po_dir);
        }

        Ok(self)
    }

    /// Preopen a directory with a different name exposed to the WASI.
    pub fn map_dir<P>(mut self, alias: &str, po_dir: P) -> Result<Self, WasiStateCreationError>
    where
        P: AsRef<Path>,
    {
        self.add_map_dir(alias, po_dir)?;
        Ok(self)
    }

    /// Preopen a directory with a different name exposed to the WASI.
    pub fn add_map_dir<P>(&mut self, alias: &str, po_dir: P) -> Result<(), WasiStateCreationError>
    where
        P: AsRef<Path>,
    {
        let mut pdb = PreopenDirBuilder::new();
        let path = po_dir.as_ref();
        pdb.directory(path)
            .alias(alias)
            .read(true)
            .write(true)
            .create(true);
        let preopen = pdb.build()?;

        self.preopens.push(preopen);

        Ok(())
    }

    /// Preopen directorys with a different names exposed to the WASI.
    pub fn map_dirs<I, P>(mut self, mapped_dirs: I) -> Result<Self, WasiStateCreationError>
    where
        I: IntoIterator<Item = (String, P)>,
        P: AsRef<Path>,
    {
        for (alias, dir) in mapped_dirs {
            self.add_map_dir(&alias, dir)?;
        }

        Ok(self)
    }

    /// Specifies one or more journal files that Wasmer will use to restore
    /// the state of the WASM process.
    ///
    /// The state of the WASM process and its sandbox will be reapplied use
    /// the journals in the order that you specify here.
    #[cfg(feature = "journal")]
    pub fn add_read_only_journal(&mut self, journal: Arc<DynReadableJournal>) {
        self.read_only_journals.push(journal);
    }

    /// Specifies one or more journal files that Wasmer will use to restore
    /// the state of the WASM process.
    ///
    /// The state of the WASM process and its sandbox will be reapplied use
    /// the journals in the order that you specify here.
    ///
    /// The last journal file specified will be created if it does not exist
    /// and opened for read and write. New journal events will be written to this
    /// file
    #[cfg(feature = "journal")]
    pub fn add_writable_journal(&mut self, journal: Arc<DynJournal>) {
        self.writable_journals.push(journal);
    }

    pub fn get_current_dir(&mut self) -> Option<PathBuf> {
        self.current_dir.clone()
    }

    pub fn set_current_dir(&mut self, dir: impl Into<PathBuf>) {
        self.current_dir = Some(dir.into());
    }

    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.set_current_dir(dir);
        self
    }

    /// Overwrite the default WASI `stdout`, if you want to hold on to the
    /// original `stdout` use [`WasiFs::swap_file`] after building.
    pub fn stdout(mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.stdout = Some(new_file);

        self
    }

    /// Overwrite the default WASI `stdout`, if you want to hold on to the
    /// original `stdout` use [`WasiFs::swap_file`] after building.
    pub fn set_stdout(&mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) {
        self.stdout = Some(new_file);
    }

    /// Overwrite the default WASI `stderr`, if you want to hold on to the
    /// original `stderr` use [`WasiFs::swap_file`] after building.
    pub fn stderr(mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.set_stderr(new_file);
        self
    }

    /// Overwrite the default WASI `stderr`, if you want to hold on to the
    /// original `stderr` use [`WasiFs::swap_file`] after building.
    pub fn set_stderr(&mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) {
        self.stderr = Some(new_file);
    }

    /// Overwrite the default WASI `stdin`, if you want to hold on to the
    /// original `stdin` use [`WasiFs::swap_file`] after building.
    pub fn stdin(mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) -> Self {
        self.stdin = Some(new_file);

        self
    }

    /// Overwrite the default WASI `stdin`, if you want to hold on to the
    /// original `stdin` use [`WasiFs::swap_file`] after building.
    pub fn set_stdin(&mut self, new_file: Box<dyn VirtualFile + Send + Sync + 'static>) {
        self.stdin = Some(new_file);
    }

    /// Sets the FileSystem to be used with this WASI instance.
    ///
    /// This is usually used in case a custom `virtual_fs::FileSystem` is needed.
    pub fn fs(mut self, fs: impl Into<Arc<dyn virtual_fs::FileSystem + Send + Sync>>) -> Self {
        self.set_fs(fs);
        self
    }

    pub fn set_fs(&mut self, fs: impl Into<Arc<dyn virtual_fs::FileSystem + Send + Sync>>) {
        self.fs = Some(WasiFsRoot::Backing(fs.into()));
    }

    pub(crate) fn set_fs_root(&mut self, fs: WasiFsRoot) {
        self.fs = Some(fs);
    }

    /// Sets a new sandbox FileSystem to be used with this WASI instance.
    ///
    /// This is usually used in case a custom `virtual_fs::FileSystem` is needed.
    pub fn sandbox_fs(mut self, fs: TmpFileSystem) -> Self {
        self.fs = Some(WasiFsRoot::Sandbox(fs));
        self
    }

    /// Configure the WASI filesystem before running.
    // TODO: improve ergonomics on this function
    pub fn setup_fs(mut self, setup_fs_fn: SetupFsFn) -> Self {
        self.setup_fs_fn = Some(setup_fs_fn);

        self
    }

    /// Sets the wasmer engine and overrides the default; only used if
    /// a runtime override is not provided.
    pub fn engine(mut self, engine: Engine) -> Self {
        self.set_engine(engine);
        self
    }

    pub fn set_engine(&mut self, engine: Engine) {
        self.engine = Some(engine);
    }

    /// Sets the WASI runtime implementation and overrides the default
    /// implementation
    pub fn runtime(mut self, runtime: Arc<dyn Runtime + Send + Sync>) -> Self {
        self.set_runtime(runtime);
        self
    }

    pub fn set_runtime(&mut self, runtime: Arc<dyn Runtime + Send + Sync>) {
        self.runtime = Some(runtime);
    }

    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.set_capabilities(capabilities);
        self
    }

    pub fn capabilities_mut(&mut self) -> &mut Capabilities {
        &mut self.capabilites
    }

    pub fn set_capabilities(&mut self, capabilities: Capabilities) {
        self.capabilites = capabilities;
    }

    #[cfg(feature = "journal")]
    pub fn add_snapshot_trigger(&mut self, on: SnapshotTrigger) {
        self.snapshot_on.push(on);
    }

    #[cfg(feature = "journal")]
    pub fn with_snapshot_interval(&mut self, interval: std::time::Duration) {
        self.snapshot_interval.replace(interval);
    }

    #[cfg(feature = "journal")]
    pub fn with_stop_running_after_snapshot(&mut self, stop_running: bool) {
        self.stop_running_after_snapshot = stop_running;
    }

    pub fn with_skip_stdio_during_bootstrap(&mut self, skip: bool) {
        self.skip_stdio_during_bootstrap = skip;
    }

    /// Consumes the [`WasiEnvBuilder`] and produces a [`WasiEnvInit`], which
    /// can be used to construct a new [`WasiEnv`].
    ///
    /// Returns the error from `WasiFs::new` if there's an error
    ///
    /// NOTE: You should prefer to not work directly with [`WasiEnvInit`].
    /// Use [`WasiEnvBuilder::build`] or [`WasiEnvBuilder::instantiate`] instead
    /// to ensure proper invokation of WASI modules.
    pub fn build_init(mut self) -> Result<WasiEnvInit, WasiStateCreationError> {
        for arg in self.args.iter() {
            for b in arg.as_bytes().iter() {
                if *b == 0 {
                    return Err(WasiStateCreationError::ArgumentContainsNulByte(arg.clone()));
                }
            }
        }

        enum InvalidCharacter {
            Nul,
            Equal,
        }

        for (env_key, env_value) in self.envs.iter() {
            match env_key.as_bytes().iter().find_map(|&ch| {
                if ch == 0 {
                    Some(InvalidCharacter::Nul)
                } else if ch == b'=' {
                    Some(InvalidCharacter::Equal)
                } else {
                    None
                }
            }) {
                Some(InvalidCharacter::Nul) => {
                    return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                        format!("found nul byte in env var key \"{env_key}\" (key=value)"),
                    ));
                }

                Some(InvalidCharacter::Equal) => {
                    return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                        format!("found equal sign in env var key \"{env_key}\" (key=value)"),
                    ));
                }

                None => (),
            }

            if env_value.contains(&0) {
                return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                    format!(
                        "found nul byte in env var value \"{}\" (key=value)",
                        String::from_utf8_lossy(env_value),
                    ),
                ));
            }
        }

        // Determine the STDIN
        let stdin: Box<dyn VirtualFile + Send + Sync + 'static> = self
            .stdin
            .take()
            .unwrap_or_else(|| Box::new(ArcFile::new(Box::<super::Stdin>::default())));

        let fs_backing = self
            .fs
            .take()
            .unwrap_or_else(|| WasiFsRoot::Sandbox(TmpFileSystem::new()));

        if let Some(dir) = &self.current_dir {
            match fs_backing.read_dir(dir) {
                Ok(_) => {
                    // All good
                }
                Err(FsError::EntryNotFound) => {
                    fs_backing.create_dir(dir).map_err(|err| {
                        WasiStateCreationError::WasiFsSetupError(format!(
                            "Could not create specified current directory at '{}': {err}",
                            dir.display()
                        ))
                    })?;
                }
                Err(err) => {
                    return Err(WasiStateCreationError::WasiFsSetupError(format!(
                        "Could not check specified current directory at '{}': {err}",
                        dir.display()
                    )));
                }
            }
        }

        // self.preopens are checked in [`PreopenDirBuilder::build`]
        let inodes = crate::state::WasiInodes::new();
        let wasi_fs = {
            // self.preopens are checked in [`PreopenDirBuilder::build`]
            let mut wasi_fs =
                WasiFs::new_with_preopen(&inodes, &self.preopens, &self.vfs_preopens, fs_backing)
                    .map_err(WasiStateCreationError::WasiFsCreationError)?;

            // set up the file system, overriding base files and calling the setup function
            wasi_fs
                .swap_file(__WASI_STDIN_FILENO, stdin)
                .map_err(WasiStateCreationError::FileSystemError)?;

            if let Some(stdout_override) = self.stdout.take() {
                wasi_fs
                    .swap_file(__WASI_STDOUT_FILENO, stdout_override)
                    .map_err(WasiStateCreationError::FileSystemError)?;
            }

            if let Some(stderr_override) = self.stderr.take() {
                wasi_fs
                    .swap_file(__WASI_STDERR_FILENO, stderr_override)
                    .map_err(WasiStateCreationError::FileSystemError)?;
            }

            if let Some(f) = &self.setup_fs_fn {
                f(&inodes, &mut wasi_fs).map_err(WasiStateCreationError::WasiFsSetupError)?;
            }
            wasi_fs
        };

        if let Some(dir) = &self.current_dir {
            let s = dir.to_str().ok_or_else(|| {
                WasiStateCreationError::WasiFsSetupError(format!(
                    "Specified current directory is not valid UTF-8: '{}'",
                    dir.display()
                ))
            })?;
            wasi_fs.set_current_dir(s);
        }

        for id in &self.included_packages {
            wasi_fs.has_unioned.lock().unwrap().insert(id.clone());
        }

        let state = WasiState {
            fs: wasi_fs,
            secret: rand::thread_rng().r#gen::<[u8; 32]>(),
            inodes,
            args: std::sync::Mutex::new(self.args.clone()),
            preopen: self.vfs_preopens.clone(),
            futexs: Default::default(),
            clock_offset: Default::default(),
            envs: std::sync::Mutex::new(conv_env_vars(self.envs)),
            signals: std::sync::Mutex::new(self.signals.iter().map(|s| (s.sig, s.disp)).collect()),
        };

        let runtime = self.runtime.unwrap_or_else(|| {
            #[cfg(feature = "sys-thread")]
            {
                #[allow(unused_mut)]
                let mut runtime = crate::runtime::PluggableRuntime::new(Arc::new(crate::runtime::task_manager::tokio::TokioTaskManager::default()));
                runtime.set_engine(
                    self
                        .engine
                        .as_ref()
                        .expect(
                            "Neither a runtime nor an engine was provided to WasiEnvBuilder. \
                            This is not supported because it means the module that's going to \
                            run with the resulting WasiEnv will have been loaded using a \
                            different engine than the one that will exist within the WasiEnv. \
                            Use either `set_runtime` or `set_engine` before calling `build_init`.",
                        )
                        .clone()
                );
                #[cfg(feature = "journal")]
                for journal in self.read_only_journals.clone() {
                    runtime.add_read_only_journal(journal);
                }
                #[cfg(feature = "journal")]
                for journal in self.writable_journals.clone() {
                    runtime.add_writable_journal(journal);
                }
                Arc::new(runtime)
            }

            #[cfg(not(feature = "sys-thread"))]
            {
                panic!("this build does not support a default runtime - specify one with WasiEnvBuilder::runtime()");
            }
        });

        let uses = self.uses;
        let map_commands = self.map_commands;

        let bin_factory = BinFactory::new(runtime.clone());

        let capabilities = self.capabilites;

        let plane_config = ControlPlaneConfig {
            max_task_count: capabilities.threading.max_threads,
            enable_asynchronous_threading: capabilities.threading.enable_asynchronous_threading,
            enable_exponential_cpu_backoff: capabilities.threading.enable_exponential_cpu_backoff,
        };
        let control_plane = WasiControlPlane::new(plane_config);

        let init = WasiEnvInit {
            state,
            runtime,
            webc_dependencies: uses,
            mapped_commands: map_commands,
            control_plane,
            bin_factory,
            capabilities,
            memory_ty: None,
            process: None,
            thread: None,
            #[cfg(feature = "journal")]
            call_initialize: self.read_only_journals.is_empty()
                && self.writable_journals.is_empty(),
            #[cfg(not(feature = "journal"))]
            call_initialize: true,
            can_deep_sleep: false,
            extra_tracing: true,
            #[cfg(feature = "journal")]
            snapshot_on: self.snapshot_on,
            #[cfg(feature = "journal")]
            stop_running_after_snapshot: self.stop_running_after_snapshot,
            skip_stdio_during_bootstrap: self.skip_stdio_during_bootstrap,
        };

        Ok(init)
    }

    #[allow(clippy::result_large_err)]
    pub fn build(self) -> Result<WasiEnv, WasiRuntimeError> {
        let module_hash = self.module_hash.unwrap_or_else(ModuleHash::random);
        let init = self.build_init()?;
        WasiEnv::from_init(init, module_hash)
    }

    /// Construct a [`WasiFunctionEnv`].
    ///
    /// NOTE: you still must call [`WasiFunctionEnv::initialize`] to make an
    /// instance usable.
    #[doc(hidden)]
    #[allow(clippy::result_large_err)]
    pub fn finalize(
        self,
        store: &mut impl AsStoreMut,
    ) -> Result<WasiFunctionEnv, WasiRuntimeError> {
        let module_hash = self.module_hash.unwrap_or_else(ModuleHash::random);
        let init = self.build_init()?;
        let env = WasiEnv::from_init(init, module_hash)?;
        let func_env = WasiFunctionEnv::new(store, env);
        Ok(func_env)
    }

    /// Consumes the [`WasiEnvBuilder`] and produces a [`WasiEnvInit`], which
    /// can be used to construct a new [`WasiEnv`].
    ///
    /// Returns the error from `WasiFs::new` if there's an error
    // FIXME: use a proper custom error type
    #[allow(clippy::result_large_err)]
    pub fn instantiate(
        self,
        module: Module,
        store: &mut impl AsStoreMut,
    ) -> Result<(Instance, WasiFunctionEnv), WasiRuntimeError> {
        self.instantiate_ext(module, ModuleHash::random(), store)
    }

    #[allow(clippy::result_large_err)]
    pub fn instantiate_ext(
        self,
        module: Module,
        module_hash: ModuleHash,
        store: &mut impl AsStoreMut,
    ) -> Result<(Instance, WasiFunctionEnv), WasiRuntimeError> {
        let init = self.build_init()?;
        let call_init = init.call_initialize;
        let env = WasiEnv::from_init(init, module_hash)?;
        let memory = module
            .imports()
            .find_map(|i| match i.ty() {
                wasmer::ExternType::Memory(ty) => Some(*ty),
                _ => None,
            })
            .map(|ty| wasmer::Memory::new(store, ty))
            .transpose()
            .map_err(WasiThreadError::MemoryCreateFailed)?;
        Ok(env.instantiate(module, store, memory, true, call_init, None)?)
    }
}

pub(crate) fn conv_env_vars(envs: Vec<(String, Vec<u8>)>) -> Vec<Vec<u8>> {
    envs.into_iter()
        .map(|(key, value)| {
            let mut env = Vec::with_capacity(key.len() + value.len() + 1);
            env.extend_from_slice(key.as_bytes());
            env.push(b'=');
            env.extend_from_slice(&value);

            env
        })
        .collect()
}

/// Builder for preopened directories.
#[derive(Debug, Default)]
pub struct PreopenDirBuilder {
    path: Option<PathBuf>,
    alias: Option<String>,
    read: bool,
    write: bool,
    create: bool,
}

/// The built version of `PreopenDirBuilder`
#[derive(Debug, Clone, Default)]
pub(crate) struct PreopenedDir {
    pub(crate) path: PathBuf,
    pub(crate) alias: Option<String>,
    pub(crate) read: bool,
    pub(crate) write: bool,
    pub(crate) create: bool,
}

impl PreopenDirBuilder {
    /// Create an empty builder
    pub(crate) fn new() -> Self {
        PreopenDirBuilder::default()
    }

    /// Point the preopened directory to the path given by `po_dir`
    pub fn directory<FilePath>(&mut self, po_dir: FilePath) -> &mut Self
    where
        FilePath: AsRef<Path>,
    {
        let path = po_dir.as_ref();
        self.path = Some(path.to_path_buf());

        self
    }

    /// Make this preopened directory appear to the WASI program as `alias`
    pub fn alias(&mut self, alias: &str) -> &mut Self {
        // We mount at preopened dirs at `/` by default and multiple `/` in a row
        // are equal to a single `/`.
        let alias = alias.trim_start_matches('/');
        self.alias = Some(alias.to_string());

        self
    }

    /// Set read permissions affecting files in the directory
    pub fn read(&mut self, toggle: bool) -> &mut Self {
        self.read = toggle;

        self
    }

    /// Set write permissions affecting files in the directory
    pub fn write(&mut self, toggle: bool) -> &mut Self {
        self.write = toggle;

        self
    }

    /// Set create permissions affecting files in the directory
    ///
    /// Create implies `write` permissions
    pub fn create(&mut self, toggle: bool) -> &mut Self {
        self.create = toggle;
        if toggle {
            self.write = true;
        }

        self
    }

    pub(crate) fn build(&self) -> Result<PreopenedDir, WasiStateCreationError> {
        // ensure at least one is set
        if !(self.read || self.write || self.create) {
            return Err(WasiStateCreationError::PreopenedDirectoryError("Preopened directories must have at least one of read, write, create permissions set".to_string()));
        }

        if self.path.is_none() {
            return Err(WasiStateCreationError::PreopenedDirectoryError(
                "Preopened directories must point to a host directory".to_string(),
            ));
        }
        let path = self.path.clone().unwrap();

        /*
        if !path.exists() {
            return Err(WasiStateCreationError::PreopenedDirectoryNotFound(path));
        }
        */

        if let Some(alias) = &self.alias {
            validate_mapped_dir_alias(alias)?;
        }

        Ok(PreopenedDir {
            path,
            alias: self.alias.clone(),
            read: self.read,
            write: self.write,
            create: self.create,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn env_var_errors() {
        #[cfg(not(target_arch = "wasm32"))]
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        #[cfg(not(target_arch = "wasm32"))]
        let handle = runtime.handle().clone();
        #[cfg(not(target_arch = "wasm32"))]
        let _guard = handle.enter();

        // `=` in the key is invalid.
        assert!(
            WasiEnv::builder("test_prog")
                .env("HOM=E", "/home/home")
                .build_init()
                .is_err(),
            "equal sign in key must be invalid"
        );

        // `\0` in the key is invalid.
        assert!(
            WasiEnvBuilder::new("test_prog")
                .env("HOME\0", "/home/home")
                .build_init()
                .is_err(),
            "nul in key must be invalid"
        );

        // `\0` in the value is invalid.
        assert!(
            WasiEnvBuilder::new("test_prog")
                .env("HOME", "/home/home\0")
                .build_init()
                .is_err(),
            "nul in value must be invalid"
        );

        // `=` in the value is valid.
        assert!(
            WasiEnvBuilder::new("test_prog")
                .env("HOME", "/home/home=home")
                .engine(Engine::default())
                .build_init()
                .is_ok(),
            "equal sign in the value must be valid"
        );
    }

    #[test]
    fn nul_character_in_args() {
        let output = WasiEnvBuilder::new("test_prog")
            .arg("--h\0elp")
            .build_init();
        let err = output.expect_err("should fail");
        assert!(matches!(
            err,
            WasiStateCreationError::ArgumentContainsNulByte(_)
        ));

        let output = WasiEnvBuilder::new("test_prog")
            .args(["--help", "--wat\0"])
            .build_init();
        let err = output.expect_err("should fail");
        assert!(matches!(
            err,
            WasiStateCreationError::ArgumentContainsNulByte(_)
        ));
    }
}
