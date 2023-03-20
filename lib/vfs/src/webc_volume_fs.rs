use std::{convert::TryInto, path::Path};

use webc::{
    compat::{Container, Volume},
    v2::{PathSegment, PathSegmentError, PathSegments, ToPathSegments},
};

use crate::{EmptyFileSystem, FileSystem, FsError, OverlayFileSystem};

#[derive(Debug, Clone)]
pub struct WebcVolumeFileSystem {
    volume: Volume,
}

impl WebcVolumeFileSystem {
    pub fn new(volume: Volume) -> Self {
        WebcVolumeFileSystem { volume }
    }

    pub fn volume(&self) -> &Volume {
        &self.volume
    }

    /// Get a filesystem where all [`Volume`]s in a [`Container`] are mounted to
    /// the root directory.
    pub fn mount_all(
        container: &Container,
    ) -> OverlayFileSystem<EmptyFileSystem, Vec<WebcVolumeFileSystem>> {
        let mut filesystems = Vec::new();

        for volume in container.volumes().into_values() {
            filesystems.push(WebcVolumeFileSystem::new(volume));
        }

        OverlayFileSystem::new(EmptyFileSystem::default(), filesystems)
    }
}

impl FileSystem for WebcVolumeFileSystem {
    fn read_dir(&self, path: &Path) -> crate::Result<crate::ReadDir> {
        let path = normalize(path).map_err(|_| FsError::InvalidInput)?;
        // self.volume.read_dir(path)
        todo!()
    }

    fn create_dir(&self, _path: &Path) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn remove_dir(&self, _path: &Path) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn rename(&self, _from: &Path, _to: &Path) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn metadata(&self, _path: &Path) -> crate::Result<crate::Metadata> {
        todo!()
    }

    fn remove_file(&self, _path: &Path) -> crate::Result<()> {
        Err(FsError::PermissionDenied)
    }

    fn new_open_options(&self) -> crate::OpenOptions {
        todo!()
    }
}

/// Normalize a [`Path`] into a [`PathSegments`], dealing with things like `..`
/// and skipping `.`'s.
#[tracing::instrument(level = "trace", err)]
fn normalize(path: &Path) -> Result<PathSegments, PathSegmentError> {
    let mut segments: Vec<PathSegment> = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::Normal(s) => {
                segments.push(s.try_into()?);
            }
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                // Note: We want /path/to/../../../../../file.txt to normalize
                // to /file.txt
                let _ = segments.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => segments.clear(),
        }
    }

    segments.to_path_segments()
}

#[cfg(test)]
mod tests {
    use crate::{DirEntry, Metadata};

    use super::*;
    const PYTHON_WEBC: &[u8] = include_bytes!("../../c-api/examples/assets/python-0.1.0.wasmer");

    #[test]
    fn mount_all_volumes_in_python() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();

        let fs = WebcVolumeFileSystem::mount_all(&container);

        let items = fs.read_dir("/".as_ref()).unwrap();
        panic!("{:?}", items);
    }

    #[test]
    fn read_dir() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();
        dbg!(volume.read_dir("/lib").unwrap());

        let fs = WebcVolumeFileSystem::new(volume);

        let entries: Vec<_> = fs
            .read_dir("/lib".as_ref())
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let expected = vec![
            DirEntry {
                path: "/lib/python.wasm".into(),
                metadata: Ok(crate::Metadata {
                    ft: crate::FileType {
                        file: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified: 0,
                    len: 1234,
                }),
            },
            DirEntry {
                path: "/lib/python3.6".into(),
                metadata: Ok(crate::Metadata {
                    ft: crate::FileType {
                        dir: true,
                        ..Default::default()
                    },
                    accessed: 0,
                    created: 0,
                    modified: 0,
                    len: 0,
                }),
            },
        ];
        todo!();
    }

    fn assert_eq_metadata(left: Metadata, right: Metadata) {
        let Metadata {
            ft,
            accessed,
            created,
            modified,
            len,
        } = left;

        assert_eq!(ft, right.ft);
        assert_eq!(accessed, right.accessed);
        assert_eq!(created, right.created);
        assert_eq!(modified, right.modified);
        assert_eq!(len, right.len);
    }

    #[test]
    fn metadata() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        let python_wasm = crate::Metadata {
            ft: crate::FileType {
                file: true,
                ..Default::default()
            },
            accessed: 0,
            created: 0,
            modified: 0,
            len: 1234,
        };
        assert_eq_metadata(
            fs.metadata("/lib/python.wasm".as_ref()).unwrap(),
            python_wasm.clone(),
        );
        assert_eq_metadata(
            fs.metadata("/../../../../lib/python.wasm".as_ref())
                .unwrap(),
            python_wasm.clone(),
        );
        assert_eq_metadata(
            fs.metadata("/lib/python3.6/../python3.6/../python.wasm".as_ref())
                .unwrap(),
            python_wasm,
        );
        assert_eq_metadata(
            fs.metadata("/lib/python3.6".as_ref()).unwrap(),
            crate::Metadata {
                ft: crate::FileType {
                    dir: true,
                    ..Default::default()
                },
                accessed: 0,
                created: 0,
                modified: 0,
                len: 0,
            },
        );
        assert_eq!(
            fs.metadata("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound
        );
    }

    #[test]
    fn file_opener() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.new_open_options()
                .create(true)
                .write(true)
                .open("/file.txt")
                .unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.new_open_options().read(true).open("/lib").unwrap_err(),
            FsError::InvalidInput,
        );
        assert_eq!(
            fs.new_open_options()
                .read(true)
                .open("/this/does/not/exist.txt")
                .unwrap_err(),
            FsError::EntryNotFound,
        );
    }

    #[test]
    fn remove_dir_is_not_allowed() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.remove_dir("/lib".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.remove_dir("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.remove_dir("/lib/python.wasm".as_ref()).unwrap_err(),
            FsError::BaseNotDirectory,
        );
    }

    #[test]
    fn remove_file_is_not_allowed() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.remove_file("/lib".as_ref()).unwrap_err(),
            FsError::NotAFile,
        );
        assert_eq!(
            fs.remove_file("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.remove_file("/lib/python.wasm".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
    }

    #[test]
    fn create_dir_is_not_allowed() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.create_dir("/lib".as_ref()).unwrap_err(),
            FsError::AlreadyExists,
        );
        assert_eq!(
            fs.create_dir("/this/does/not/exist".as_ref()).unwrap_err(),
            FsError::BaseNotDirectory,
        );
        assert_eq!(
            fs.remove_file("/lib/nested/".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
    }

    #[test]
    fn rename_is_not_allowed() {
        let container = Container::from_bytes(PYTHON_WEBC).unwrap();
        let volumes = container.volumes();
        let volume = volumes["atom"].clone();

        let fs = WebcVolumeFileSystem::new(volume);

        assert_eq!(
            fs.rename("/lib".as_ref(), "/other".as_ref()).unwrap_err(),
            FsError::PermissionDenied,
        );
        assert_eq!(
            fs.rename("/this/does/not/exist".as_ref(), "/another".as_ref())
                .unwrap_err(),
            FsError::EntryNotFound,
        );
        assert_eq!(
            fs.rename("/lib/python.wasm".as_ref(), "/lib/another.wasm".as_ref())
                .unwrap_err(),
            FsError::PermissionDenied,
        );
    }
}
