//! Builder code for [`WasiState`]

use crate::state::{WasiFs, WasiState};
use std::path::{Path, PathBuf};

pub fn create_wasi_state() -> WasiStateBuilder {
    WasiStateBuilder::default()
}

/// Type for building an instance of [`WasiState`]
///
/// Usage:
///
/// ```
/// # use wasmer_wasi::state::create_wasi_state;
/// let build_wasi_state = || {
///     create_wasi_state()
///         .add_env_pair(b"HOME", "/home/home".to_string())?
///         .add_arg("--help")?
///         .add_env_pairs(&[("COLOR_OUTPUT", "TRUE"), ("PATH", "/usr/bin")])?
///         .add_args(&["--verbose", "list"])?
///         .add_preopened_dir("src")?
///         .add_mapped_dir("dot", ".")
/// };
/// let wasi_file_system = build_wasi_state().unwrap().build().unwrap();
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WasiStateBuilder {
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<PathBuf>,
    mapped_dirs: Vec<(String, PathBuf)>,
}

/// Error type returned when bad data is given to [`WasiStateBuilder`].
#[derive(Debug, PartialEq, Eq)]
pub enum WasiStateCreationError {
    EnvironmentVariableFormatError(String),
    ArgumentContainsNulByte,
    PreopenedDirectoryNotFound,
    MappedDirAliasFormattingError(String),
}

fn validate_env_var(key: &[u8], val: &[u8]) -> Result<(), WasiStateCreationError> {
    for key_byte in key.iter() {
        match *key_byte {
            b'=' => {
                return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                    "Key contains the `=` byte".to_string(),
                ))
            }
            b'\0' => {
                return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                    "Key contains null byte".to_string(),
                ))
            }
            _ => (),
        }
    }

    for val_byte in val.iter() {
        match *val_byte {
            b'=' => {
                return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                    "Value contains the `=` byte".to_string(),
                ))
            }
            b'\0' => {
                return Err(WasiStateCreationError::EnvironmentVariableFormatError(
                    "Value contains null byte".to_string(),
                ))
            }
            _ => (),
        }
    }

    Ok(())
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

impl WasiStateBuilder {
    /// Add an environment variable pair.
    /// Environment variable keys and values must not contain the byte `=` (0x3d)
    /// or nul (0x0).
    pub fn add_env_pair<Key, Value>(
        mut self,
        key: Key,
        value: Value,
    ) -> Result<Self, WasiStateCreationError>
    where
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        let key_b = key.as_ref();
        let val_b = value.as_ref();
        validate_env_var(key_b, val_b)?;

        let length = key_b.len() + val_b.len() + 1;
        let mut byte_vec = Vec::with_capacity(length);

        byte_vec.extend_from_slice(&key_b);
        byte_vec.push(b'=');
        byte_vec.extend_from_slice(&val_b);

        self.envs.push(byte_vec);

        Ok(self)
    }

    /// Add an argument.
    /// Arguments must not contain the nul (0x0) byte
    pub fn add_arg<Arg>(mut self, arg: Arg) -> Result<Self, WasiStateCreationError>
    where
        Arg: AsRef<[u8]>,
    {
        let arg_b = arg.as_ref();
        for byte in arg_b.iter() {
            if *byte == 0 {
                return Err(WasiStateCreationError::ArgumentContainsNulByte);
            }
        }

        let mut byte_vec = Vec::with_capacity(arg_b.len());
        byte_vec.extend_from_slice(&arg_b);
        self.args.push(byte_vec);

        Ok(self)
    }

    /// Add multiple environment variable pairs.
    /// Keys and values must not contain the `=` (0x3d) or nul (0x0) byte.
    pub fn add_env_pairs<Key, Value>(
        mut self,
        env_pairs: &[(Key, Value)],
    ) -> Result<Self, WasiStateCreationError>
    where
        Key: AsRef<[u8]>,
        Value: AsRef<[u8]>,
    {
        for (key, value) in env_pairs.iter() {
            let key_b = key.as_ref();
            let val_b = value.as_ref();
            validate_env_var(key_b, val_b)?;

            let length = key_b.len() + val_b.len() + 1;
            let mut byte_vec = Vec::with_capacity(length);

            byte_vec.extend_from_slice(&key_b);
            byte_vec.push(b'=');
            byte_vec.extend_from_slice(&val_b);

            self.envs.push(byte_vec);
        }

        Ok(self)
    }

    /// Add multiple arguments.
    /// Arguments must not contain the nul (0x0) byte
    pub fn add_args<Arg>(mut self, args: &[Arg]) -> Result<Self, WasiStateCreationError>
    where
        Arg: AsRef<[u8]>,
    {
        for arg in args.iter() {
            let arg_b = arg.as_ref();
            for byte in arg_b.iter() {
                if *byte == 0 {
                    return Err(WasiStateCreationError::ArgumentContainsNulByte);
                }
            }

            let mut byte_vec = Vec::with_capacity(arg_b.len());
            byte_vec.extend_from_slice(&arg_b);
            self.args.push(byte_vec);
        }

        Ok(self)
    }

    /// Preopen a directory
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    // TODO: design a simple API for passing in permissions here (i.e. read-only)
    pub fn add_preopened_dir<FilePath>(
        mut self,
        po_dir: FilePath,
    ) -> Result<Self, WasiStateCreationError>
    where
        FilePath: AsRef<Path>,
    {
        let path = po_dir.as_ref();
        if !path.exists() {
            return Err(WasiStateCreationError::PreopenedDirectoryNotFound);
        }
        self.preopened_files.push(path.to_path_buf());

        Ok(self)
    }

    /// Preopen a directory
    /// This opens the given directory at the virtual root, `/`, and allows
    /// the WASI module to read and write to the given directory.
    pub fn add_preopened_dirs<FilePath>(
        mut self,
        po_dirs: &[FilePath],
    ) -> Result<Self, WasiStateCreationError>
    where
        FilePath: AsRef<Path>,
    {
        for po_dir in po_dirs.iter() {
            let path = po_dir.as_ref();
            if !path.exists() {
                return Err(WasiStateCreationError::PreopenedDirectoryNotFound);
            }
            self.preopened_files.push(path.to_path_buf());
        }

        Ok(self)
    }

    /// Preopen a directory with a different name exposed to the WASI.
    pub fn add_mapped_dir<FilePath>(
        mut self,
        alias: &str,
        po_dir: FilePath,
    ) -> Result<Self, WasiStateCreationError>
    where
        FilePath: AsRef<Path>,
    {
        let path = po_dir.as_ref();
        validate_mapped_dir_alias(alias)?;
        if !path.exists() {
            return Err(WasiStateCreationError::PreopenedDirectoryNotFound);
        }
        self.mapped_dirs
            .push((alias.to_string(), path.to_path_buf()));

        Ok(self)
    }

    /// Consumes the [`WasiStateBuilder`] and produces a [`WasiState`]
    ///
    /// Returns the error from `WasiFs::new` if there's an error
    pub fn build(self) -> Result<WasiState, String> {
        Ok(WasiState {
            fs: WasiFs::new(&self.preopened_files, &self.mapped_dirs)?,
            args: self.args,
            envs: self.envs,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn env_var_errors() {
        let output = create_wasi_state().add_env_pair("HOM=E", "/home/home");
        match output {
            Err(WasiStateCreationError::EnvironmentVariableFormatError(_)) => assert!(true),
            _ => assert!(false),
        }

        let output = create_wasi_state().add_env_pair("HOME\0", "/home/home");
        match output {
            Err(WasiStateCreationError::EnvironmentVariableFormatError(_)) => assert!(true),
            _ => assert!(false),
        }
    }

    #[test]
    fn nul_character_in_args() {
        let output = create_wasi_state().add_arg("--h\0elp");
        assert_eq!(output, Err(WasiStateCreationError::ArgumentContainsNulByte));
        let output = create_wasi_state().add_args(&["--help", "--wat\0"]);
        assert_eq!(output, Err(WasiStateCreationError::ArgumentContainsNulByte));
    }
}
