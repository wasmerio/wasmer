#![allow(clippy::result_large_err)]
use std::{
    collections::HashMap,
    future::Future,
    ops::Deref,
    path::Path,
    pin::Pin,
    sync::{Arc, RwLock},
};

use anyhow::Context;
use shared_buffer::OwnedBuffer;
use virtual_fs::{AsyncReadExt, FileSystem};
use wasmer::FunctionEnvMut;
use wasmer_package::utils::from_bytes;

mod binary_package;
mod exec;

pub use self::{
    binary_package::*,
    exec::{
        import_package_mounts, package_command_by_name, run_exec, spawn_exec, spawn_exec_module,
        spawn_exec_wasm, spawn_load_module,
    },
};
use crate::{
    Runtime, SpawnError, VIRTUAL_ROOT_FD, WasiEnv,
    fs::Kind,
    os::{
        command::{Commands, VirtualCommand},
        task::TaskJoinHandle,
    },
    runtime::module_cache::HashedModuleData,
};

#[derive(Debug, Clone)]
pub struct BinFactory {
    pub(crate) commands: Commands,
    runtime: Arc<dyn Runtime + Send + Sync + 'static>,
    pub(crate) local: Arc<RwLock<HashMap<String, Option<Arc<BinaryPackage>>>>>,
}

impl BinFactory {
    pub fn new(runtime: Arc<dyn Runtime + Send + Sync + 'static>) -> BinFactory {
        BinFactory {
            commands: Commands::new_with_builtins(runtime.clone()),
            runtime,
            local: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn runtime(&self) -> &(dyn Runtime + Send + Sync) {
        self.runtime.deref()
    }

    /// Register a builtin command.
    pub fn register_builtin_command<C>(&mut self, cmd: C)
    where
        C: VirtualCommand + Send + Sync + 'static,
    {
        self.commands.register_command(cmd);
    }

    /// Register a builtin command at a custom path.
    pub fn register_builtin_command_with_path<C, P>(&mut self, cmd: C, path: P)
    where
        C: VirtualCommand + Send + Sync + 'static,
        P: Into<String>,
    {
        self.commands.register_command_with_path(cmd, path.into());
    }

    /// Register a builtin command behind an [`Arc`] at a custom path.
    pub(crate) fn register_builtin_command_with_path_shared<P>(
        &mut self,
        cmd: Arc<dyn VirtualCommand + Send + Sync + 'static>,
        path: P,
    ) where
        P: Into<String>,
    {
        self.commands
            .register_command_with_path_shared(cmd, path.into());
    }

    /// Remove all registered builtin commands.
    pub fn clear_builtin_commands(&mut self) {
        self.commands.clear();
    }

    pub fn set_binary(&self, name: &str, binary: &Arc<BinaryPackage>) {
        let mut cache = self.local.write().unwrap();
        cache.insert(name.to_string(), Some(binary.clone()));
    }

    #[allow(clippy::await_holding_lock)]
    pub async fn get_binary(
        &self,
        name: &str,
        fs: Option<&dyn FileSystem>,
    ) -> Option<Arc<BinaryPackage>> {
        self.get_executable(name, fs)
            .await
            .and_then(|executable| match executable {
                Executable::Wasm(_) | Executable::Script(_) => None,
                Executable::BinaryPackage(pkg) => Some(pkg),
            })
    }

    /// `invoked_as` is the path the guest spelled, for a script that must be
    /// handed to its interpreter under that name rather than the path it
    /// resolved to. `None` selects the resolved path, which is what a PATH
    /// search produces on Unix.
    pub fn spawn<'a>(
        &'a self,
        name: String,
        invoked_as: Option<String>,
        env: WasiEnv,
    ) -> Pin<Box<dyn Future<Output = Result<TaskJoinHandle, SpawnError>> + 'a>> {
        Box::pin(async move {
            let mut name = name;
            let mut invoked_as = invoked_as;

            // A shebang is handled by the kernel on Unix. WASIX's binary factory
            // fills that role for virtual filesystems, so resolve scripts here
            // before trying to compile their bytes as WebAssembly.
            for _ in 0..MAX_SHEBANG_DEPTH {
                let (resolved_name, executable) = self
                    .get_executable_for_spawn(name.as_str(), &env)
                    .await?
                    .ok_or_else(|| SpawnError::BinaryNotFound {
                        binary: name.clone(),
                    })?;
                name = resolved_name;

                match executable {
                    Executable::Wasm(bytes) => {
                        let data = HashedModuleData::new(bytes.clone());
                        return spawn_exec_wasm(data, name.as_str(), env, &self.runtime).await;
                    }
                    Executable::BinaryPackage(pkg) => {
                        {
                            let cmd = package_command_by_name(&pkg, name.as_str())?;
                            env.prepare_spawn(cmd);
                        }

                        return spawn_exec(pkg.as_ref().clone(), name.as_str(), env, &self.runtime)
                            .await;
                    }
                    Executable::Script(script) => {
                        let script_path = invoked_as.unwrap_or_else(|| name.clone());
                        name = prepare_script_execution(&env, &script_path, script);
                        // The next round resolves the interpreter, which the
                        // shebang line named directly.
                        invoked_as = Some(name.clone());
                    }
                }
            }

            Err(SpawnError::InvalidABI)
        })
    }

    pub fn try_built_in(
        &self,
        name: String,
        parent_ctx: Option<&FunctionEnvMut<'_, WasiEnv>>,
        builder: &mut Option<WasiEnv>,
    ) -> Result<TaskJoinHandle, SpawnError> {
        // We check for built in commands
        if let Some(parent_ctx) = parent_ctx {
            if self.commands.exists(name.as_str()) {
                return self.commands.exec(parent_ctx, name.as_str(), builder);
            }
        } else if self.commands.exists(name.as_str()) {
            tracing::warn!("builtin command without a parent ctx - {}", name);
        }
        Err(SpawnError::BinaryNotFound { binary: name })
    }

    // TODO: remove allow once BinFactory is refactored
    // currently fine because a BinFactory is only used by a single process tree
    #[allow(clippy::await_holding_lock)]
    pub async fn get_executable(
        &self,
        name: &str,
        fs: Option<&dyn FileSystem>,
    ) -> Option<Executable> {
        let name = name.to_string();

        // Return early if the path is already cached
        {
            let cache = self.local.read().unwrap();
            if let Some(data) = cache.get(&name) {
                return data.clone().map(Executable::BinaryPackage);
            }
        }

        let mut cache = self.local.write().unwrap();

        // Check the cache again to avoid a race condition where the cache was populated inbetween the fast path and here
        if let Some(data) = cache.get(&name) {
            return data.clone().map(Executable::BinaryPackage);
        }

        // Check the filesystem for the file
        if name.starts_with('/')
            && let Some(fs) = fs
        {
            match load_executable_from_filesystem(fs, name.as_ref(), self.runtime()).await {
                Ok(executable) => {
                    if let Executable::BinaryPackage(pkg) = &executable {
                        cache.insert(name, Some(pkg.clone()));
                    }

                    return Some(executable);
                }
                Err(e) => {
                    tracing::warn!(
                        path = name,
                        error = &*e,
                        "Unable to load the package from disk"
                    );
                }
            }
        }

        // Do not negatively cache filesystem lookups: package managers and
        // running guests can create executables after an earlier miss.
        None
    }

    async fn get_executable_for_spawn(
        &self,
        name: &str,
        env: &WasiEnv,
    ) -> Result<Option<(String, Executable)>, SpawnError> {
        if name.contains('/') {
            let name = if name.starts_with('/') {
                name.to_string()
            } else {
                env.state.fs.relative_path_to_absolute(name.to_string())
            };
            let executable = self.get_executable_from_wasi_fs(&name, env).await?;
            return Ok(executable.map(|executable| (name, executable)));
        }

        for directory in executable_search_path(env) {
            let path = if directory.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", directory.trim_end_matches('/'), name)
            };
            let path = if path.starts_with('/') {
                path
            } else {
                env.state.fs.relative_path_to_absolute(path)
            };
            if let Some(executable) = self.get_executable_from_wasi_fs(&path, env).await? {
                return Ok(Some((path, executable)));
            }
        }

        Ok(None)
    }

    async fn get_executable_from_wasi_fs(
        &self,
        path: &str,
        env: &WasiEnv,
    ) -> Result<Option<Executable>, SpawnError> {
        if let Some(binary) = self.local.read().unwrap().get(path).cloned().flatten() {
            return Ok(Some(Executable::BinaryPackage(binary)));
        }

        match load_executable_from_wasi_fs(env, Path::new(path), self.runtime()).await {
            Ok(executable) => {
                if let Executable::BinaryPackage(package) = &executable {
                    self.local
                        .write()
                        .unwrap()
                        .insert(path.to_string(), Some(package.clone()));
                }
                Ok(Some(executable))
            }
            // A script whose shebang cannot be used is not a missing file, and
            // continuing the PATH walk would report the wrong error.
            Err(error) if error.downcast_ref::<MalformedShebang>().is_some() => {
                Err(SpawnError::InvalidShebang {
                    path: path.to_string(),
                })
            }
            Err(error) => {
                tracing::debug!(path, error = &*error, "Unable to load executable");
                Ok(None)
            }
        }
    }
}

fn executable_search_path(env: &WasiEnv) -> Vec<String> {
    env.state
        .envs
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|entry| {
            entry
                .strip_prefix(b"PATH=")
                .map(|path| String::from_utf8_lossy(path).into_owned())
        })
        .unwrap_or_else(|| "/usr/local/bin:/bin:/usr/bin".to_string())
        .split(':')
        .map(str::to_string)
        .collect()
}

pub enum Executable {
    Wasm(OwnedBuffer),
    BinaryPackage(Arc<BinaryPackage>),
    Script(Shebang),
}

const MAX_SHEBANG_DEPTH: usize = 4;

/// Linux reads the shebang out of a buffer of `BINPRM_BUF_SIZE` bytes and
/// refuses to exec an interpreter path that the buffer truncated.
const MAX_SHEBANG_LINE: usize = 256;

#[derive(Debug)]
pub struct Shebang {
    interpreter: String,
    argument: Option<String>,
}

/// A file that starts with `#!` is a script and nothing else: Linux commits to
/// the script handler on those two bytes and never falls back to another
/// format. A line this runtime cannot use is therefore ENOEXEC, not something
/// to hand to the Wasm compiler.
#[derive(Debug, thiserror::Error)]
#[error("the shebang line cannot be used")]
struct MalformedShebang;

enum ShebangLine {
    Absent,
    Malformed,
    Parsed(Shebang),
}

fn parse_shebang(bytes: &[u8]) -> ShebangLine {
    if !bytes.starts_with(b"#!") {
        return ShebangLine::Absent;
    }

    let window = &bytes[..bytes.len().min(MAX_SHEBANG_LINE)];
    let line = match window.iter().position(|byte| *byte == b'\n') {
        Some(end) => &window[..end],
        // Without a newline the line ends where the file does, so anything
        // reaching the end of the window is truncated.
        None if bytes.len() < MAX_SHEBANG_LINE => window,
        None => return ShebangLine::Malformed,
    };

    // WASIX resolves paths as UTF-8 strings. Linux takes them as bytes, so a
    // path this runtime cannot represent is reported rather than ignored.
    let Ok(line) = std::str::from_utf8(&line[2..]) else {
        return ShebangLine::Malformed;
    };
    let line = line.trim_end_matches('\r').trim();

    let (interpreter, argument) = line
        .split_once(char::is_whitespace)
        .map(|(interpreter, argument)| (interpreter, Some(argument.trim().to_string())))
        .unwrap_or((line, None));

    if interpreter.is_empty() {
        return ShebangLine::Malformed;
    }

    ShebangLine::Parsed(Shebang {
        interpreter: interpreter.to_string(),
        argument: argument.filter(|argument| !argument.is_empty()),
    })
}

fn prepare_script_execution(env: &WasiEnv, script_name: &str, script: Shebang) -> String {
    let mut args = env.state.args.lock().unwrap();
    let (interpreter, new_args) = script_command(script_name, script, &args);
    *args = new_args;
    interpreter
}

fn script_command(
    script_name: &str,
    script: Shebang,
    original_args: &[String],
) -> (String, Vec<String>) {
    let user_args = original_args.iter().skip(1).cloned();

    // As on Unix, whatever follows the interpreter on the shebang line is a
    // single argument. `/usr/bin/env` needs no special case: it resolves like
    // any other interpreter, and the `env` shipped by `wasmer/coreutils` does
    // its own `-S` splitting.
    let args = std::iter::once(script.interpreter.clone())
        .chain(script.argument)
        .chain(std::iter::once(script_name.to_string()))
        .chain(user_args)
        .collect();

    (script.interpreter, args)
}

async fn load_executable_from_filesystem(
    fs: &dyn FileSystem,
    path: &Path,
    rt: &(dyn Runtime + Send + Sync),
) -> Result<Executable, anyhow::Error> {
    let mut f = fs
        .new_open_options()
        .read(true)
        .open(path)
        .context("Unable to open the file")?;

    // Fast path if the file is fully available in memory. This prevents a
    // redundant copy and keeps executable classification in one place.
    if let Some(buf) = f.as_owned_buffer() {
        load_executable_from_buffer(buf, rt).await
    } else {
        let mut data = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut data).await.context("Read failed")?;
        load_executable_from_buffer(OwnedBuffer::from_bytes(data), rt).await
    }
}

async fn load_executable_from_wasi_fs(
    env: &WasiEnv,
    path: &Path,
    rt: &(dyn Runtime + Send + Sync),
) -> Result<Executable, anyhow::Error> {
    // Resolving through the inode tree follows symlinks and reaches a file that
    // only exists in memory, but it sees only what a preopen covers. Without a
    // matching one the virtual root has nothing to offer, so fall back to
    // opening the root filesystem, which is where executables were found
    // before. That fallback does not follow symlinks.
    let inode = match env.state.fs.get_inode_at_path(
        &env.state.inodes,
        VIRTUAL_ROOT_FD,
        path.to_string_lossy().as_ref(),
        true,
    ) {
        Ok(inode) => inode,
        Err(error) => {
            tracing::debug!(
                %error,
                path = path.to_string_lossy().as_ref(),
                "Unable to resolve executable through the preopens",
            );
            return load_executable_from_filesystem(env.fs_root(), path, rt).await;
        }
    };

    let (buffer, backing_path) = {
        let kind = inode.read();
        match &*kind {
            Kind::File { handle, path, .. } => {
                let buffer = handle
                    .as_ref()
                    .and_then(|handle| handle.read().unwrap().as_owned_buffer());
                (buffer, path.clone())
            }
            _ => anyhow::bail!("Executable is not a regular file"),
        }
    };

    if let Some(buffer) = buffer {
        load_executable_from_buffer(buffer, rt).await
    } else {
        load_executable_from_filesystem(&env.state.fs.root_fs, &backing_path, rt).await
    }
}

async fn load_executable_from_buffer(
    buffer: OwnedBuffer,
    rt: &(dyn Runtime + Send + Sync),
) -> Result<Executable, anyhow::Error> {
    if wasmer_package::utils::is_container(buffer.as_slice()) {
        let bytes = buffer.clone().into_bytes();
        if let Ok(container) = from_bytes(bytes) {
            let package = BinaryPackage::from_webc(&container, rt)
                .await
                .context("Unable to load the package")?;
            return Ok(Executable::BinaryPackage(Arc::new(package)));
        }
    }

    match parse_shebang(buffer.as_slice()) {
        ShebangLine::Parsed(script) => Ok(Executable::Script(script)),
        ShebangLine::Malformed => Err(MalformedShebang.into()),
        ShebangLine::Absent => Ok(Executable::Wasm(buffer)),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use virtual_fs::{AsyncWriteExt, FileSystem};
    use wasmer::Engine;

    use super::{
        Executable, MAX_SHEBANG_LINE, Shebang, ShebangLine, load_executable_from_wasi_fs,
        parse_shebang, script_command,
    };
    use crate::{VIRTUAL_ROOT_FD, WasiEnvBuilder};

    fn expect_parsed(line: ShebangLine) -> Shebang {
        match line {
            ShebangLine::Parsed(script) => script,
            ShebangLine::Absent => panic!("expected a shebang"),
            ShebangLine::Malformed => panic!("expected a usable shebang"),
        }
    }

    #[test]
    fn parses_env_shebang() {
        let script = expect_parsed(parse_shebang(
            b"#!/usr/bin/env node\nconsole.log('hello')\n",
        ));
        assert_eq!(script.interpreter, "/usr/bin/env");
        assert_eq!(script.argument.as_deref(), Some("node"));
    }

    #[test]
    fn parses_direct_shebang_with_crlf() {
        let script = expect_parsed(parse_shebang(b"#!/bin/bash -e\r\necho hello\r\n"));
        assert_eq!(script.interpreter, "/bin/bash");
        assert_eq!(script.argument.as_deref(), Some("-e"));
    }

    #[test]
    fn ignores_regular_files() {
        assert!(matches!(
            parse_shebang(b"console.log('hello')\n"),
            ShebangLine::Absent
        ));
    }

    #[test]
    fn a_line_longer_than_the_kernel_buffer_is_rejected() {
        // Linux fits the whole line, `#!` and newline included, in
        // BINPRM_BUF_SIZE and refuses to exec a path the buffer truncated.
        let fits = format!("#!/{}\n", "y".repeat(MAX_SHEBANG_LINE - 4));
        assert_eq!(fits.len(), MAX_SHEBANG_LINE);
        assert!(matches!(
            parse_shebang(fits.as_bytes()),
            ShebangLine::Parsed(_)
        ));

        let one_too_long = format!("#!/{}\n", "y".repeat(MAX_SHEBANG_LINE - 3));
        assert_eq!(one_too_long.len(), MAX_SHEBANG_LINE + 1);
        assert!(matches!(
            parse_shebang(one_too_long.as_bytes()),
            ShebangLine::Malformed
        ));
    }

    #[test]
    fn a_short_line_needs_no_newline() {
        let Shebang {
            interpreter,
            argument,
        } = expect_parsed(parse_shebang(b"#!/bin/echo hi"));
        assert_eq!(interpreter, "/bin/echo");
        assert_eq!(argument.as_deref(), Some("hi"));
    }

    #[test]
    fn a_non_utf8_interpreter_is_rejected_rather_than_compiled() {
        // Linux runs this: paths there are bytes. WASIX resolves paths as
        // UTF-8, so the honest answer is ENOEXEC, never a Wasm compile error.
        assert!(matches!(
            parse_shebang(b"#!/usr/bin/int\xff\xfebad\n"),
            ShebangLine::Malformed
        ));
    }

    #[test]
    fn an_empty_interpreter_is_rejected() {
        assert!(matches!(parse_shebang(b"#!\n"), ShebangLine::Malformed));
        assert!(matches!(parse_shebang(b"#!   \n"), ShebangLine::Malformed));
    }

    #[test]
    fn script_keeps_the_path_the_caller_spelled() {
        // execv("./tool", ...) on Linux hands the interpreter "./tool", not the
        // path the kernel resolved it to.
        let script = expect_parsed(parse_shebang(b"#!/bin/interp\n"));
        let original = vec!["./tool".to_string(), "arg".to_string()];
        let (interpreter, args) = script_command("./tool", script, &original);

        assert_eq!(interpreter, "/bin/interp");
        assert_eq!(args, ["/bin/interp", "./tool", "arg"]);
    }

    #[test]
    fn env_shebang_execs_env_itself() {
        let script = expect_parsed(parse_shebang(b"#!/usr/bin/env node\n"));
        let original = vec!["next".to_string(), "dev".to_string()];
        let (interpreter, args) = script_command("/workspace/.bin/next", script, &original);

        assert_eq!(interpreter, "/usr/bin/env");
        assert_eq!(
            args,
            ["/usr/bin/env", "node", "/workspace/.bin/next", "dev"]
        );
    }

    #[test]
    fn env_split_string_stays_one_argument() {
        let script = expect_parsed(parse_shebang(b"#!/usr/bin/env -S node --no-warnings\n"));
        let original = vec!["tool".to_string(), "input.js".to_string()];
        let (interpreter, args) = script_command("/workspace/tool", script, &original);

        assert_eq!(interpreter, "/usr/bin/env");
        assert_eq!(
            args,
            [
                "/usr/bin/env",
                "-S node --no-warnings",
                "/workspace/tool",
                "input.js"
            ]
        );
    }

    #[test]
    fn direct_shebang_inserts_optional_argument_before_script() {
        let script = expect_parsed(parse_shebang(b"#!/bin/bash -e\n"));
        let original = vec!["script".to_string(), "hello".to_string()];
        let (interpreter, args) = script_command("/workspace/script", script, &original);

        assert_eq!(interpreter, "/bin/bash");
        assert_eq!(args, ["/bin/bash", "-e", "/workspace/script", "hello"]);
    }

    #[tokio::test]
    async fn loads_shebang_through_relative_symlink() {
        let mut builder = WasiEnvBuilder::new("test").engine(Engine::default());
        builder.preopen_vfs_dirs(["/".to_string()]).unwrap();
        let env = builder.build().unwrap();
        let fs = &env.state.fs.root_fs;
        fs.create_dir(Path::new("/bin")).unwrap();
        fs.create_dir(Path::new("/pkg")).unwrap();

        let mut target = fs
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/pkg/next"))
            .unwrap();
        target
            .write_all(b"#!/usr/bin/env node\nconsole.log('hello')\n")
            .await
            .unwrap();
        fs.create_symlink(Path::new("../pkg/next"), Path::new("/bin/next"))
            .unwrap();

        let executable = load_executable_from_wasi_fs(&env, Path::new("/bin/next"), env.runtime())
            .await
            .unwrap();
        assert!(matches!(executable, Executable::Script(_)));
    }

    #[tokio::test]
    async fn loads_an_executable_outside_every_preopen() {
        // An embedder that preopens only its own directory still gets to exec
        // what it wrote elsewhere; the inode tree offers nothing there, since
        // the root holds preopens alone.
        let backing = virtual_fs::mem_fs::FileSystem::default();
        backing.create_dir(Path::new("/app")).unwrap();
        backing.create_dir(Path::new("/bin")).unwrap();

        let mut builder = WasiEnvBuilder::new("test")
            .engine(Engine::default())
            .fs(Arc::new(backing) as Arc<dyn FileSystem + Send + Sync>);
        builder.preopen_vfs_dirs(["/app".to_string()]).unwrap();
        let env = builder.build().unwrap();
        let fs = &env.state.fs.root_fs;

        let mut target = fs
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/bin/tool"))
            .unwrap();
        target.write_all(b"#!/bin/sh\necho hello\n").await.unwrap();

        assert!(
            env.state
                .fs
                .get_inode_at_path(&env.state.inodes, VIRTUAL_ROOT_FD, "/bin/tool", true)
                .is_err(),
            "the preopens should not cover this path, or the test proves nothing"
        );

        let executable = load_executable_from_wasi_fs(&env, Path::new("/bin/tool"), env.runtime())
            .await
            .unwrap();
        assert!(matches!(executable, Executable::Script(_)));
    }

    #[tokio::test]
    async fn a_symlink_outside_every_preopen_is_not_followed() {
        // Documents today's boundary rather than endorsing it: the fallback
        // opens the root filesystem, which stops at the link itself. Widen this
        // test if that ever changes.
        let backing = virtual_fs::mem_fs::FileSystem::default();
        backing.create_dir(Path::new("/app")).unwrap();
        backing.create_dir(Path::new("/bin")).unwrap();
        backing.create_dir(Path::new("/pkg")).unwrap();

        let mut builder = WasiEnvBuilder::new("test")
            .engine(Engine::default())
            .fs(Arc::new(backing) as Arc<dyn FileSystem + Send + Sync>);
        builder.preopen_vfs_dirs(["/app".to_string()]).unwrap();
        let env = builder.build().unwrap();
        let fs = &env.state.fs.root_fs;

        let mut target = fs
            .new_open_options()
            .create(true)
            .write(true)
            .open(Path::new("/pkg/tool"))
            .unwrap();
        target.write_all(b"#!/bin/sh\necho hello\n").await.unwrap();
        fs.create_symlink(Path::new("../pkg/tool"), Path::new("/bin/tool"))
            .unwrap();

        assert!(
            load_executable_from_wasi_fs(&env, Path::new("/bin/tool"), env.runtime())
                .await
                .is_err()
        );

        // The file the link points at is reachable directly, so only the
        // symlink hop is what fails.
        let executable = load_executable_from_wasi_fs(&env, Path::new("/pkg/tool"), env.runtime())
            .await
            .unwrap();
        assert!(matches!(executable, Executable::Script(_)));
    }
}
