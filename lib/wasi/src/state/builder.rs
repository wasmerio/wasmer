//! Builder system for configuring a [`WasiState`] and creating it.

use crate::state::{WasiFile, WasiFs, WasiFsError, WasiState};
use crate::syscalls::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};
use std::path::{Path, PathBuf};

/// Creates an empty [`WasiStateBuilder`].
///
/// Internal method only, users should call [`WasiState::new`].
pub(crate) fn create_wasi_state(program_name: &str) -> WasiStateBuilder {
    WasiStateBuilder {
        args: vec![program_name.bytes().collect()],
        ..WasiStateBuilder::default()
    }
}

/// Convenient builder API for configuring WASI via [`WasiState`].
///
/// Usage:
/// ```no_run
/// # use wasmer_wasi::state::{WasiState, WasiStateCreationError};
/// # fn main() -> Result<(), WasiStateCreationError> {
/// let mut state_builder = WasiState::new("wasi-prog-name");
/// state_builder
///    .env("ENV_VAR", "ENV_VAL")
///    .arg("--verbose")
///    .preopen_dir("src")?
///    .map_dir("name_wasi_sees", "path/on/host/fs")?
///    .build();
/// # Ok(())
/// # }
/// ```
#[derive(Default)]
pub struct WasiStateBuilder {
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopens: Vec<PreopenedDir>,
    setup_fs_fn: Option<Box<dyn Fn(&mut WasiFs) -> Result<(), String> + Send>>,
    stdout_override: Option<Box<dyn WasiFile>>,
    stderr_override: Option<Box<dyn WasiFile>>,
    stdin_override: Option<Box<dyn WasiFile>>,
}

impl std::fmt::Debug for WasiStateBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasiStateBuilder")
            .field("args", &self.args)
            .field("envs", &self.envs)
            .field("preopens", &self.preopens)
            .field("setup_fs_fn exists", &self.setup_fs_fn.is_some())
            .field("stdout_override exists", &self.stdout_override.is_some())
            .field("stderr_override exists", &self.stderr_override.is_some())
            .field("stdin_override exists", &self.stdin_override.is_some())
            .finish()
    }
}

/// Error type returned when bad data is given to [`WasiStateBuilder`].
#[derive(Debug, PartialEq, Eq)]
pub enum WasiStateCreationError {
    EnvironmentVariableFormatError(String),
    ArgumentContainsNulByte(String),
    PreopenedDirectoryNotFound(PathBuf),
    PreopenedDirectoryError(String),
    MappedDirAliasFormattingError(String),
    WasiFsCreationError(String),
    WasiFsSetupError(String),
    WasiFsError(WasiFsError),
}

fn validate_mapped_dir_alias(alias: &str) -> Result<(), WasiStateCreationError> {
    for byte in alias.bytes() {
        match byte {
            b'\0' => {
                return Err(WasiStateCreationError::MappedDirAliasFormattingError(
                    format!("Alias \"{}\" contains a nul byte", alias),
                ));
            }
            _ => (),
        }
    }

    Ok(())
}

// TODO add other WasiFS APIs here like swapping out stdout, for example (though we need to
// return stdout somehow, it's unclear what that API should look like)
impl WasiStateBuilder {
    /// Add an environment variable pair.
    /// Environment variable keys and values must not contain the byte `=` (0x3d)
    /// or nul (0x0).
    pub fn env<Key, Value>(&mut self, key: Key, value: Value) -> &mut Self
    where
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        let key_b = key.as_ref();
        let val_b = value.as_ref();

        let length = key_b.len() + val_b.len() + 1;
        let mut byte_vec = Vec::with_capacity(length);

        byte_vec.extend_from_slice(&key_b);
        byte_vec.push(b'=');
        byte_vec.extend_from_slice(&val_b);

        self.envs.push(byte_vec);

        self
    }

    /// Add an argument.
    /// Arguments must not contain the nul (0x0) byte
    pub fn arg<Arg>(&mut self, arg: Arg) -> &mut Self
    where
        Arg: AsRef<[u8]>,
    {
        let arg_b = arg.as_ref();
        let mut byte_vec = Vec::with_capacity(arg_b.len());
        byte_vec.extend_from_slice(&arg_b);
        self.args.push(byte_vec);

        self
    }

    /// Add multiple environment variable pairs.
    /// Keys and values must not contain the `=` (0x3d) or nul (0x0) byte.
    pub fn envs<I, Key, Value>(&mut self, env_pairs: I) -> &mut Self
    where
        I: IntoIterator<Item = (Key, Value)>,
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        for (key, value) in env_pairs {
            let key_b = key.as_ref();
            let val_b = value.as_ref();

            let length = key_b.len() + val_b.len() + 1;
            let mut byte_vec = Vec::with_capacity(length);

            byte_vec.extend_from_slice(&key_b);
            byte_vec.push(b'=');
            byte_vec.extend_from_slice(&val_b);

            self.envs.push(byte_vec);
        }

        self
    }

    /// Add multiple arguments.
    /// Arguments must not contain the nul (0x0) byte
    pub fn args<I, Arg>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = Arg>,
        Arg: AsRef<[u8]>,
    {
        for arg in args {
            let arg_b = arg.as_ref();
            let mut byte_vec = Vec::with_capacity(arg_b.len());
            byte_vec.extend_from_slice(&arg_b);
            self.args.push(byte_vec);
        }

        self
    }

    /// Preopen a directory
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn preopen_dir<FilePath>(
        &mut self,
        po_dir: FilePath,
    ) -> Result<&mut Self, WasiStateCreationError>
    where
        FilePath: AsRef<Path>,
    {
        let mut pdb = PreopenDirBuilder::new();
        let path = po_dir.as_ref();
        pdb.directory(path).read(true).write(true).create(true);
        let preopen = pdb.build()?;

        self.preopens.push(preopen);

        Ok(self)
    }

    /// Preopen a directory and configure it.
    ///
    /// Usage:
    ///
    /// ```no_run
    /// # use wasmer_wasi::state::{WasiState, WasiStateCreationError};
    /// # fn main() -> Result<(), WasiStateCreationError> {
    /// WasiState::new("program_name")
    ///    .preopen(|p| p.directory("src").read(true).write(true).create(true))?
    ///    .preopen(|p| p.directory(".").alias("dot").read(true))?
    ///    .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn preopen<F>(&mut self, inner: F) -> Result<&mut Self, WasiStateCreationError>
    where
        F: Fn(&mut PreopenDirBuilder) -> &mut PreopenDirBuilder,
    {
        let mut pdb = PreopenDirBuilder::new();
        let po_dir = inner(&mut pdb).build()?;

        self.preopens.push(po_dir);

        Ok(self)
    }

    /// Preopen a directory
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn preopen_dirs<I, FilePath>(
        &mut self,
        po_dirs: I,
    ) -> Result<&mut Self, WasiStateCreationError>
    where
        I: IntoIterator<Item = FilePath>,
        FilePath: AsRef<Path>,
    {
        for po_dir in po_dirs {
            self.preopen_dir(po_dir)?;
        }

        Ok(self)
    }

    /// Preopen a directory with a different name exposed to the WASI.
    pub fn map_dir<FilePath>(
        &mut self,
        alias: &str,
        po_dir: FilePath,
    ) -> Result<&mut Self, WasiStateCreationError>
    where
        FilePath: AsRef<Path>,
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

        Ok(self)
    }

    /// Preopen directorys with a different names exposed to the WASI.
    pub fn map_dirs<I, FilePath>(
        &mut self,
        mapped_dirs: I,
    ) -> Result<&mut Self, WasiStateCreationError>
    where
        I: IntoIterator<Item = (String, FilePath)>,
        FilePath: AsRef<Path>,
    {
        for (alias, dir) in mapped_dirs {
            self.map_dir(&alias, dir)?;
        }

        Ok(self)
    }

    /// Overwrite the default WASI `stdout`, if you want to hold on to the
    /// original `stdout` use [`WasiFs::swap_file`] after building.
    pub fn stdout(&mut self, new_file: Box<dyn WasiFile>) -> &mut Self {
        self.stdout_override = Some(new_file);

        self
    }

    /// Overwrite the default WASI `stderr`, if you want to hold on to the
    /// original `stderr` use [`WasiFs::swap_file`] after building.
    pub fn stderr(&mut self, new_file: Box<dyn WasiFile>) -> &mut Self {
        self.stderr_override = Some(new_file);

        self
    }

    /// Overwrite the default WASI `stdin`, if you want to hold on to the
    /// original `stdin` use [`WasiFs::swap_file`] after building.
    pub fn stdin(&mut self, new_file: Box<dyn WasiFile>) -> &mut Self {
        self.stdin_override = Some(new_file);

        self
    }

    /// Setup the WASI filesystem before running
    // TODO: improve ergonomics on this function
    pub fn setup_fs(
        &mut self,
        setup_fs_fn: Box<dyn Fn(&mut WasiFs) -> Result<(), String> + Send>,
    ) -> &mut Self {
        self.setup_fs_fn = Some(setup_fs_fn);

        self
    }

    /// Consumes the [`WasiStateBuilder`] and produces a [`WasiState`]
    ///
    /// Returns the error from `WasiFs::new` if there's an error
    pub fn build(&mut self) -> Result<WasiState, WasiStateCreationError> {
        for (i, arg) in self.args.iter().enumerate() {
            for b in arg.iter() {
                if *b == 0 {
                    return Err(WasiStateCreationError::ArgumentContainsNulByte(
                        std::str::from_utf8(arg)
                            .unwrap_or(if i == 0 {
                                "Inner error: program name is invalid utf8!"
                            } else {
                                "Inner error: arg is invalid utf8!"
                            })
                            .to_string(),
                    ));
                }
            }
        }
        for env in self.envs.iter() {
            let mut eq_seen = false;
            for b in env.iter() {
                match *b {
                    b'=' => {
                        if eq_seen {
                            return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                                format!(
                                    "found '=' in env var string \"{}\" (key=value)",
                                    std::str::from_utf8(env)
                                        .unwrap_or("Inner error: env var is invalid_utf8!")
                                ),
                            ));
                        }
                        eq_seen = true;
                    }
                    0 => {
                        return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                            format!(
                                "found nul byte in env var string \"{}\" (key=value)",
                                std::str::from_utf8(env)
                                    .unwrap_or("Inner error: env var is invalid_utf8!")
                            ),
                        ));
                    }
                    _ => (),
                }
            }
        }

        // self.preopens are checked in [`PreopenDirBuilder::build`]

        // this deprecation warning only applies to external callers
        #[allow(deprecated)]
        let mut wasi_fs = WasiFs::new_with_preopen(&self.preopens)
            .map_err(WasiStateCreationError::WasiFsCreationError)?;
        // set up the file system, overriding base files and calling the setup function
        if let Some(stdin_override) = self.stdin_override.take() {
            wasi_fs
                .swap_file(__WASI_STDIN_FILENO, stdin_override)
                .map_err(WasiStateCreationError::WasiFsError)?;
        }
        if let Some(stdout_override) = self.stdout_override.take() {
            wasi_fs
                .swap_file(__WASI_STDOUT_FILENO, stdout_override)
                .map_err(WasiStateCreationError::WasiFsError)?;
        }
        if let Some(stderr_override) = self.stderr_override.take() {
            wasi_fs
                .swap_file(__WASI_STDERR_FILENO, stderr_override)
                .map_err(WasiStateCreationError::WasiFsError)?;
        }
        if let Some(f) = &self.setup_fs_fn {
            f(&mut wasi_fs).map_err(WasiStateCreationError::WasiFsSetupError)?;
        }
        Ok(WasiState {
            fs: wasi_fs,
            args: self.args.clone(),
            envs: self.envs.clone(),
        })
    }
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
#[derive(Debug, Default)]
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

        if !path.exists() {
            return Err(WasiStateCreationError::PreopenedDirectoryNotFound(path));
        }
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
        let output = create_wasi_state("test_prog")
            .env("HOM=E", "/home/home")
            .build();
        match output {
            Err(WasiStateCreationError::EnvironmentVariableFormatError(_)) => assert!(true),
            _ => assert!(false),
        }

        let output = create_wasi_state("test_prog")
            .env("HOME\0", "/home/home")
            .build();
        match output {
            Err(WasiStateCreationError::EnvironmentVariableFormatError(_)) => assert!(true),
            _ => assert!(false),
        }
    }

    #[test]
    fn nul_character_in_args() {
        let output = create_wasi_state("test_prog").arg("--h\0elp").build();
        match output {
            Err(WasiStateCreationError::ArgumentContainsNulByte(_)) => assert!(true),
            _ => assert!(false),
        }
        let output = create_wasi_state("test_prog")
            .args(&["--help", "--wat\0"])
            .build();
        match output {
            Err(WasiStateCreationError::ArgumentContainsNulByte(_)) => assert!(true),
            _ => assert!(false),
        }
    }
}
