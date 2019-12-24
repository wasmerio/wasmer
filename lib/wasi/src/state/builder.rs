//! Builder system for configuring a [`WasiState`] and creating it.

use crate::state::{WasiFs, WasiState};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Creates an empty [`WasiStateBuilder`].
pub(crate) fn create_wasi_state(program_name: &str) -> WasiStateBuilder {
    WasiStateBuilder {
        args: vec![program_name.bytes().collect()],
        ..WasiStateBuilder::default()
    }
}

/// Type for building an instance of [`WasiState`]
#[derive(Default, Clone)]
pub struct WasiStateBuilder {
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<PathBuf>,
    mapped_dirs: Vec<(String, PathBuf)>,
    setup_fs_fn: Option<Rc<dyn Fn(&mut WasiFs) -> Result<(), String> + Send>>,
}

impl std::fmt::Debug for WasiStateBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasiStateBuilder")
            .field("args", &self.args)
            .field("envs", &self.envs)
            .field("preopend_files", &self.preopened_files)
            .field("mapped_dirs", &self.mapped_dirs)
            .field("setup_fs_fn exists", &self.setup_fs_fn.is_some())
            .finish()
    }
}

/// Error type returned when bad data is given to [`WasiStateBuilder`].
#[derive(Debug, PartialEq, Eq)]
pub enum WasiStateCreationError {
    EnvironmentVariableFormatError(String),
    ArgumentContainsNulByte(String),
    PreopenedDirectoryNotFound(PathBuf),
    MappedDirAliasFormattingError(String),
    WasiFsCreationError(String),
    WasiFsSetupError(String),
}

fn validate_mapped_dir_alias(alias: &str) -> Result<(), WasiStateCreationError> {
    for byte in alias.bytes() {
        match byte {
            b'/' => {
                return Err(WasiStateCreationError::MappedDirAliasFormattingError(
                    format!("Alias \"{}\" contains the character '/'", alias),
                ));
            }
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
    // TODO: design a simple API for passing in permissions here (i.e. read-only)
    pub fn preopen_dir<FilePath>(&mut self, po_dir: FilePath) -> &mut Self
    where
        FilePath: AsRef<Path>,
    {
        let path = po_dir.as_ref();
        self.preopened_files.push(path.to_path_buf());

        self
    }

    /// Preopen a directory
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn preopen_dirs<I, FilePath>(&mut self, po_dirs: I) -> &mut Self
    where
        I: IntoIterator<Item = FilePath>,
        FilePath: AsRef<Path>,
    {
        for po_dir in po_dirs {
            let path = po_dir.as_ref();
            self.preopened_files.push(path.to_path_buf());
        }

        self
    }

    /// Preopen a directory with a different name exposed to the WASI.
    pub fn map_dir<FilePath>(&mut self, alias: &str, po_dir: FilePath) -> &mut Self
    where
        FilePath: AsRef<Path>,
    {
        let path = po_dir.as_ref();
        self.mapped_dirs
            .push((alias.to_string(), path.to_path_buf()));

        self
    }

    /// Preopen directorys with a different names exposed to the WASI.
    pub fn map_dirs<I, FilePath>(&mut self, mapped_dirs: I) -> &mut Self
    where
        I: IntoIterator<Item = (String, FilePath)>,
        FilePath: AsRef<Path>,
    {
        for (alias, dir) in mapped_dirs {
            let path = dir.as_ref();
            self.mapped_dirs.push((alias, path.to_path_buf()));
        }

        self
    }

    /// Setup the WASI filesystem before running
    // TODO: improve ergonomics on this function
    pub fn setup_fs(
        &mut self,
        setup_fs_fn: Rc<dyn Fn(&mut WasiFs) -> Result<(), String> + Send>,
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

        for po_f in self.preopened_files.iter() {
            if !po_f.exists() {
                return Err(WasiStateCreationError::PreopenedDirectoryNotFound(
                    po_f.clone(),
                ));
            }
        }

        for (alias, po_f) in self.mapped_dirs.iter() {
            if !po_f.exists() {
                return Err(WasiStateCreationError::PreopenedDirectoryNotFound(
                    po_f.clone(),
                ));
            }
            validate_mapped_dir_alias(&alias)?;
        }
        let mut wasi_fs = WasiFs::new(&self.preopened_files, &self.mapped_dirs)
            .map_err(WasiStateCreationError::WasiFsCreationError)?;
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
