use std::{
    borrow::Cow,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use shared_buffer::OwnedBuffer;
use tracing::trace;
use virtual_fs::{AsyncReadExt, FileSystem, FsError};

use crate::{WasiFs, fs::WasiFsRoot};

use super::{LinkError, LocateModuleError};

const DEFAULT_RUNTIME_PATH: [&str; 3] = ["/lib", "/usr/lib", "/usr/local/lib"];

pub(super) async fn locate_module(
    module_path: &Path,
    library_path: &[impl AsRef<Path>],
    runtime_path: &[impl AsRef<str>],
    calling_module_path: Option<impl AsRef<Path>>,
    fs: &WasiFs,
) -> Result<(PathBuf, OwnedBuffer), LinkError> {
    async fn try_load(
        fs: &WasiFsRoot,
        path: impl AsRef<Path>,
    ) -> Result<(PathBuf, OwnedBuffer), FsError> {
        let mut file = match fs.new_open_options().read(true).open(path.as_ref()).await {
            Ok(f) => f,
            // Fallback for cases where the module thinks it's running on unix,
            // but the compiled side module is a .wasm file
            Err(_) if path.as_ref().extension() == Some(OsStr::new("so")) => fs
                .new_open_options()
                .read(true)
                .open(path.as_ref().with_extension("wasm"))
                .await?,
            Err(e) => return Err(e),
        };

        let buf = if let Some(buf) = file.as_owned_buffer().await {
            buf
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await?;
            OwnedBuffer::from(buf)
        };

        Ok((path.as_ref().to_owned(), buf))
    }

    if module_path.is_absolute() {
        trace!(?module_path, "Locating module with absolute path");
        try_load(&fs.root_fs, module_path).await.map_err(|e| {
            LinkError::SharedLibraryMissing(
                module_path.to_string_lossy().into_owned(),
                LocateModuleError::Single(e),
            )
        })
    } else if module_path.components().count() > 1 {
        trace!(?module_path, "Locating module with relative path");
        try_load(
            &fs.root_fs,
            fs.relative_path_to_absolute(module_path.to_string_lossy().into_owned()),
        )
        .await
        .map_err(|e| {
            LinkError::SharedLibraryMissing(
                module_path.to_string_lossy().into_owned(),
                LocateModuleError::Single(e),
            )
        })
    } else {
        // Go through all dynamic library lookup paths
        // Note: a path without a slash does *not* look at the current directory. This is by design.

        trace!(
            ?module_path,
            "Locating module by name in default runtime path"
        );

        let calling_module_dir = calling_module_path
            .as_ref()
            .map(|p| p.as_ref().parent().unwrap_or_else(|| p.as_ref()));

        let runtime_path = runtime_path.iter().map(|path| {
            let path = path.as_ref();

            let relative = path
                .strip_prefix("$ORIGIN")
                .or_else(|| path.strip_prefix("${ORIGIN}"));

            match relative {
                Some(relative) => {
                    let Some(calling_module_dir) = calling_module_dir else {
                        // This is an internal error because the only time calling_module_path
                        // should be empty is when loading a module through dlopen, and a
                        // dlopen'ed module isn't being required by another module so we don't
                        // have a RUNPATH to consider at all. See the invocation of
                        // `load_module_tree` in `load_module`.
                        panic!(
                            "Internal error: $ORIGIN or ${{ORIGIN}} in RUNPATH, but \
                            no calling module path provided"
                        );
                    };
                    Cow::Owned(PathBuf::from(
                        fs.relative_path_to_absolute(
                            calling_module_dir
                                .join(relative)
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    ))
                }
                None => Cow::Borrowed(Path::new(path)),
            }
        });

        // Search order is: LD_LIBRARY_PATH -> RUNPATH -> system default folders
        let search_paths = library_path
            .iter()
            .map(|path| Cow::Borrowed(path.as_ref()))
            .chain(runtime_path)
            .chain(
                DEFAULT_RUNTIME_PATH
                    .iter()
                    .map(|path| Cow::Borrowed(Path::new(path))),
            );

        let mut errors: Vec<(PathBuf, FsError)> = Vec::new();
        for path in search_paths {
            let full_path = path.join(module_path);
            trace!(search_path = ?path, full_path = ?full_path, "Searching module");
            match try_load(&fs.root_fs, &full_path).await {
                Ok(ret) => {
                    trace!(?module_path, full_path = ?ret.0, "Located module");
                    return Ok(ret);
                }
                Err(e) => errors.push((full_path, e)),
            };
        }

        trace!(?module_path, "Failed to locate module");
        Err(LinkError::SharedLibraryMissing(
            module_path.to_string_lossy().into_owned(),
            LocateModuleError::Multiple(errors),
        ))
    }
}
