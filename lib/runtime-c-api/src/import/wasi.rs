use super::*;
use crate::get_slice_checked;
use libc::c_uchar;
use std::{path::PathBuf, ptr, str};
use wasmer_wasi as wasi;

#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum Version {
    /// Version cannot be detected or is unknown.
    Unknown = 0,

    /// Latest version. See `wasmer_wasi::WasiVersion::Latest` to
    /// leran more.
    Latest = 1,

    /// `wasi_unstable`.
    Snapshot0 = 2,

    /// `wasi_snapshot_preview1`.
    Snapshot1 = 3,
}

impl From<c_uchar> for Version {
    fn from(value: c_uchar) -> Self {
        match value {
            1 => Self::Latest,
            2 => Self::Snapshot0,
            3 => Self::Snapshot1,
            _ => Self::Unknown,
        }
    }
}

/// Opens a directory that's visible to the WASI module as `alias` but
/// is backed by the host file at `host_file_path`
#[repr(C)]
pub struct wasmer_wasi_map_dir_entry_t {
    /// What the WASI module will see in its virtual root
    pub alias: wasmer_byte_array,
    /// The backing file that the WASI module will interact with via the alias
    pub host_file_path: wasmer_byte_array,
}

impl wasmer_wasi_map_dir_entry_t {
    /// Converts the data into owned, Rust types
    pub unsafe fn as_tuple(&self) -> Result<(String, PathBuf), str::Utf8Error> {
        let alias = self.alias.as_str()?.to_owned();
        let host_path = PathBuf::from(self.host_file_path.as_str()?);

        Ok((alias, host_path))
    }
}

/// Creates a WASI import object.
///
/// This function treats null pointers as empty collections.
/// For example, passing null for a string in `args`, will lead to a zero
/// length argument in that position.
#[no_mangle]
pub unsafe extern "C" fn wasmer_wasi_generate_import_object(
    args: *const wasmer_byte_array,
    args_len: c_uint,
    envs: *const wasmer_byte_array,
    envs_len: c_uint,
    preopened_files: *const wasmer_byte_array,
    preopened_files_len: c_uint,
    mapped_dirs: *const wasmer_wasi_map_dir_entry_t,
    mapped_dirs_len: c_uint,
) -> *mut wasmer_import_object_t {
    let arg_list = get_slice_checked(args, args_len as usize);
    let env_list = get_slice_checked(envs, envs_len as usize);
    let preopened_file_list = get_slice_checked(preopened_files, preopened_files_len as usize);
    let mapped_dir_list = get_slice_checked(mapped_dirs, mapped_dirs_len as usize);

    wasmer_wasi_generate_import_object_inner(
        Version::Latest,
        arg_list,
        env_list,
        preopened_file_list,
        mapped_dir_list,
    )
    .unwrap_or(ptr::null_mut())
}

/// Creates a WASI import object for a specific version.
///
/// This function is similar to `wasmer_wasi_generate_import_object`
/// except that the first argument describes the WASI version.
///
/// The version is expected to be of kind `Version`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_wasi_generate_import_object_for_version(
    version: c_uchar,
    args: *const wasmer_byte_array,
    args_len: c_uint,
    envs: *const wasmer_byte_array,
    envs_len: c_uint,
    preopened_files: *const wasmer_byte_array,
    preopened_files_len: c_uint,
    mapped_dirs: *const wasmer_wasi_map_dir_entry_t,
    mapped_dirs_len: c_uint,
) -> *mut wasmer_import_object_t {
    let arg_list = get_slice_checked(args, args_len as usize);
    let env_list = get_slice_checked(envs, envs_len as usize);
    let preopened_file_list = get_slice_checked(preopened_files, preopened_files_len as usize);
    let mapped_dir_list = get_slice_checked(mapped_dirs, mapped_dirs_len as usize);

    wasmer_wasi_generate_import_object_inner(
        version.into(),
        arg_list,
        env_list,
        preopened_file_list,
        mapped_dir_list,
    )
    .unwrap_or(ptr::null_mut())
}

/// Find the version of WASI used by the module.
///
/// In case of error, the returned version is `Version::Unknown`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_wasi_get_version(module: *const wasmer_module_t) -> Version {
    if module.is_null() {
        return Version::Unknown;
    }

    let module = &*(module as *const Module);

    match wasi::get_wasi_version(module, false) {
        Some(version) => match version {
            wasi::WasiVersion::Snapshot0 => Version::Snapshot0,
            wasi::WasiVersion::Snapshot1 => Version::Snapshot1,
            wasi::WasiVersion::Latest => Version::Latest,
        },
        None => Version::Unknown,
    }
}

/// Inner function that wraps error handling
fn wasmer_wasi_generate_import_object_inner(
    version: Version,
    arg_list: &[wasmer_byte_array],
    env_list: &[wasmer_byte_array],
    preopened_file_list: &[wasmer_byte_array],
    mapped_dir_list: &[wasmer_wasi_map_dir_entry_t],
) -> Result<*mut wasmer_import_object_t, str::Utf8Error> {
    let arg_vec = arg_list.iter().map(|arg| unsafe { arg.as_vec() }).collect();
    let env_vec = env_list
        .iter()
        .map(|env_var| unsafe { env_var.as_vec() })
        .collect();
    let po_file_vec = preopened_file_list
        .iter()
        .map(|po_file| Ok(unsafe { PathBuf::from(po_file.as_str()?) }.to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    let mapped_dir_vec = mapped_dir_list
        .iter()
        .map(|entry| unsafe { entry.as_tuple() })
        .collect::<Result<Vec<_>, _>>()?;

    let version = match version {
        Version::Latest => wasi::WasiVersion::Latest,
        Version::Snapshot0 => wasi::WasiVersion::Snapshot0,
        Version::Snapshot1 => wasi::WasiVersion::Snapshot1,
        _ => panic!("Version {:?} is invalid.", version),
    };

    let import_object = Box::new(wasi::generate_import_object_for_version(
        version,
        arg_vec,
        env_vec,
        po_file_vec,
        mapped_dir_vec,
    ));
    Ok(Box::into_raw(import_object) as *mut wasmer_import_object_t)
}

/// Convenience function that creates a WASI import object with no arguments,
/// environment variables, preopened files, or mapped directories.
///
/// This function is the same as calling [`wasmer_wasi_generate_import_object`] with all
/// empty values.
#[no_mangle]
pub unsafe extern "C" fn wasmer_wasi_generate_default_import_object() -> *mut wasmer_import_object_t
{
    let import_object = Box::new(wasi::generate_import_object(vec![], vec![], vec![], vec![]));

    Box::into_raw(import_object) as *mut wasmer_import_object_t
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn test_versions_from_uint() {
        assert_eq!(Version::Unknown, 0.into());
        assert_eq!(Version::Latest, 1.into());
        assert_eq!(Version::Snapshot0, 2.into());
        assert_eq!(Version::Snapshot1, 3.into());
    }
}
