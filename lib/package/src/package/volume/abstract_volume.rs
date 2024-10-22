use std::{fmt::Debug, sync::Arc};

use shared_buffer::OwnedBuffer;

use webc::{PathSegment, PathSegmentError, PathSegments, Timestamps, ToPathSegments};

use crate::container::ContainerError;

/// A WEBC volume.
///
/// A `Volume` represents a collection of files and directories, providing
/// methods to read file contents and traverse directories.
///
/// # Example
///
/// ```
/// #[cfg(not(feature = "v3"))]
/// # fn main() {}
/// #[cfg(feature = "v3")]
/// # fn main() {
/// use webc::{Metadata, Volume};
/// # use webc::{
/// #     compat::Container,
/// #     PathSegment,
/// #     v3::{
/// #         write::{Directory, Writer},
/// #         read::OwnedReader,
/// #         SignatureAlgorithm,
/// #         Timestamps
/// #     },
/// # };
/// # use sha2::Digest;
///
/// fn get_webc_volume() -> Volume {
///     /* ... */
/// # let dir = Directory::new(
/// #   [
/// #       (
/// #           PathSegment::parse("path").unwrap(),
/// #           Directory::new(
/// #               [
/// #                   (
/// #                       PathSegment::parse("to").unwrap(),
/// #                       Directory::new(
/// #                           [
/// #                               (PathSegment::parse("file.txt").unwrap(), b"Hello, World!".to_vec().into()),
/// #                           ].into_iter().collect(),
/// #                           Timestamps::default(),
/// #                       ).into(),
/// #                   ),
/// #               ].into_iter().collect(),
///                 Timestamps::default(),
/// #           ).into(),
/// #       ),
/// #       (
/// #           PathSegment::parse("another.txt").unwrap(),
/// #           b"Another".to_vec().into(),
/// #       ),
/// #   ].into_iter().collect(),
/// #   Timestamps::default(),
/// # );
/// # let serialized = Writer::default()
/// #     .write_manifest(&webc::metadata::Manifest::default()).unwrap()
/// #     .write_atoms(std::collections::BTreeMap::new()).unwrap()
/// #     .with_volume("my_volume", dir).unwrap()
/// #     .finish(SignatureAlgorithm::None).unwrap();
/// # Container::from_bytes(serialized).unwrap().get_volume("my_volume").unwrap()
/// }
/// let another_hash: [u8; 32] = sha2::Sha256::digest(b"Another").into();
/// let file_hash: [u8; 32] = sha2::Sha256::digest(b"Hello, World!").into();
/// let to_hash: [u8; 32] = sha2::Sha256::digest(&file_hash).into();
/// let path_hash: [u8; 32] = sha2::Sha256::digest(&to_hash).into();
///
/// let volume = get_webc_volume();
/// // Accessing file content.
/// let (content, hash) = volume.read_file("/path/to/file.txt").unwrap();
/// assert_eq!(content, b"Hello, World!");
/// assert_eq!(hash, Some(file_hash));
/// // Inspect directories.
/// let timestamps = Some(webc::Timestamps::default());
/// let entries = volume.read_dir("/").unwrap();
///
/// assert_eq!(entries.len(), 2);
/// assert_eq!(entries[0], (
///     PathSegment::parse("another.txt").unwrap(),
///     Some(another_hash),
///     Metadata::File { length: 7, timestamps },
/// ));
/// assert_eq!(entries[1], (
///     PathSegment::parse("path").unwrap(),
///     Some(path_hash),
///     Metadata::Dir { timestamps },
/// ));
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Volume {
    imp: Arc<dyn AbstractVolume + Send + Sync + 'static>,
}

impl Volume {
    #[allow(dead_code)]
    pub(crate) fn new(volume: impl AbstractVolume + Send + Sync + 'static) -> Self {
        Volume {
            imp: Arc::new(volume),
        }
    }

    /// Get the metadata of an item at the given path.
    ///
    /// Returns `None` if the item does not exist in the volume or an internal
    /// error occurred.
    pub fn metadata(&self, path: impl ToPathSegments) -> Option<Metadata> {
        let path = path.to_path_segments().ok()?;
        self.imp.metadata(&path)
    }

    /// Read the contents of a directory at the given path.
    ///
    /// Returns a vector of directory entries, including their metadata, if the
    /// path is a directory.
    ///
    /// Returns `None` if the path does not exist or is not a directory.
    pub fn read_dir(
        &self,
        path: impl ToPathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        let path = path.to_path_segments().ok()?;
        self.imp.read_dir(&path)
    }

    /// Read the contents of a file at the given path.
    ///
    /// Returns `None` if the path is not valid or the file is not found.
    pub fn read_file(&self, path: impl ToPathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        let path = path.to_path_segments().ok()?;
        self.imp.read_file(&path)
    }

    /// Unpack a subdirectory of this volume into a local directory.
    ///
    /// Use '/' as the volume_path to unpack the entire volume.
    #[allow(clippy::result_large_err)]
    pub fn unpack(
        &self,
        volume_path: impl ToPathSegments,
        out_dir: &std::path::Path,
    ) -> Result<(), ContainerError> {
        std::fs::create_dir_all(out_dir).map_err(|err| ContainerError::Open {
            path: out_dir.to_path_buf(),
            error: err,
        })?;

        let path = volume_path.to_path_segments()?;

        for (name, _, entry) in self.read_dir(&path).unwrap_or_default() {
            match entry {
                Metadata::Dir { .. } => {
                    let out_nested = out_dir.join(name.as_str());
                    self.unpack(path.join(name), &out_nested)?;
                }
                Metadata::File { .. } => {
                    let out_path = out_dir.join(name.as_str());
                    let p = path.join(name.clone());

                    if let Some((f, _)) = self.read_file(p) {
                        std::fs::write(&out_path, f.as_slice()).map_err(|err| {
                            ContainerError::Open {
                                path: out_path,
                                error: err,
                            }
                        })?;
                    }
                }
            }
        }

        Ok(())
    }
}

/// Metadata describing the properties of a file or directory.
#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Metadata {
    /// A directory
    Dir {
        /// Timestamps fo the directory
        timestamps: Option<Timestamps>,
    },
    /// A file with a specified length.
    File {
        /// The number of bytes in this file.
        length: usize,
        /// Timestamps of the file
        timestamps: Option<Timestamps>,
    },
}

impl Metadata {
    /// Returns `true` if the metadata represents a directory.
    pub fn is_dir(self) -> bool {
        matches!(self, Metadata::Dir { .. })
    }

    /// Returns `true` if the metadata represents a file.
    pub fn is_file(self) -> bool {
        matches!(self, Metadata::File { .. })
    }

    /// Returns the timestamps of the directory or file.
    pub fn timestamps(&self) -> Option<Timestamps> {
        let timestamps = match self {
            Metadata::Dir { timestamps } => timestamps,
            Metadata::File { timestamps, .. } => timestamps,
        };

        *timestamps
    }

    /// Returnes mutable ref to the timestamps of the directory or file.
    pub fn timestamps_mut(&mut self) -> Option<&mut Timestamps> {
        let timestamps = match self {
            Metadata::Dir { timestamps } => timestamps.as_mut(),
            Metadata::File { timestamps, .. } => timestamps.as_mut(),
        };

        timestamps
    }
}

pub(crate) trait AbstractVolume: Debug {
    fn metadata(&self, path: &PathSegments) -> Option<Metadata>;
    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>>;
    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)>;
}

impl AbstractVolume for webc::v2::read::VolumeSection {
    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        let entry = self.find_entry(path)?;
        Some(v2_metadata(&entry))
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        let meta = self.find_entry(path).and_then(|entry| entry.into_dir())?;

        let mut entries = Vec::new();

        for (name, entry) in meta.entries().flatten() {
            let segment: PathSegment = name.parse().unwrap();
            let meta = v2_metadata(&entry);
            entries.push((segment, None, meta));
        }

        Some(entries)
    }

    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        self.lookup_file(path).map(|b| (b, None)).ok()
    }
}

fn v2_metadata(header_entry: &webc::v2::read::HeaderEntry<'_>) -> Metadata {
    match header_entry {
        webc::v2::read::HeaderEntry::Directory(_) => Metadata::Dir { timestamps: None },
        webc::v2::read::HeaderEntry::File(metadata) => {
            let (start_offset, end_offset) = metadata.range();

            Metadata::File {
                length: end_offset - start_offset,
                timestamps: None,
            }
        }
    }
}

impl AbstractVolume for webc::v3::read::VolumeSection {
    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        let (entry, _) = self.find_entry(path)?;
        Some(v3_metadata(&entry))
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        let meta = self
            .find_entry(path)
            .and_then(|(entry, _)| entry.into_dir())?;

        let mut entries = Vec::new();

        for (name, hash, entry) in meta.entries().flatten() {
            let segment: PathSegment = name.parse().unwrap();
            let meta = v3_metadata(&entry);
            entries.push((segment, Some(hash), meta));
        }

        Some(entries)
    }

    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        self.lookup_file(path).map(|(b, h)| (b, Some(h))).ok()
    }
}

fn v3_metadata(header_entry: &webc::v3::read::HeaderEntry<'_>) -> Metadata {
    match header_entry {
        webc::v3::read::HeaderEntry::Directory(dir) => Metadata::Dir {
            timestamps: Some(dir.timestamps().into()),
        },
        webc::v3::read::HeaderEntry::File(metadata) => {
            let (start_offset, end_offset) = metadata.range();

            Metadata::File {
                length: end_offset - start_offset,
                timestamps: Some((metadata.timestamps()).into()),
            }
        }
    }
}

/// Errors that may occur when doing [`Volume`] operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VolumeError {
    /// The item wasn't found.
    #[error("The item wasn't found")]
    NotFound,
    /// The provided path wasn't valid.
    #[error("Invalid path")]
    Path(#[from] PathSegmentError),
    /// A non-directory was found where a directory was expected.
    #[error("Not a directory")]
    NotADirectory,
    /// A non-file was found where a file was expected.
    #[error("Not a file")]
    NotAFile,
}

#[cfg(test)]
mod tests {
    use sha2::Digest;

    use std::collections::BTreeMap;
    use webc::{metadata::Manifest, v3::write::Writer, PathSegment, Timestamps};

    use crate::package::volume::abstract_volume::{Metadata, Volume};

    macro_rules! dir_map {
    ( $( $key:expr => $value:expr ),* $(,)?) => {{
        let children = [
                $(
                    ($key.parse().unwrap(), webc::v3::write::DirEntry::from($value))
                ),*
            ]
            .into_iter()
            .collect();

        webc::v3::write::Directory::new(
            children,
            webc::v3::Timestamps::default()
        )
    }};
}

    fn v3_volume(volume: webc::v3::write::Directory<'static>) -> webc::v3::read::VolumeSection {
        let manifest = Manifest::default();
        let mut writer = Writer::default()
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::new())
            .unwrap();
        writer.write_volume("volume", volume).unwrap();
        let serialized = writer.finish(webc::v3::SignatureAlgorithm::None).unwrap();
        let reader = webc::v3::read::OwnedReader::parse(serialized).unwrap();
        reader.get_volume("volume").unwrap()
    }

    #[test]
    fn v3() {
        let dir = dir_map! {
            "path" => dir_map! {
                "to" => dir_map! {
                    "file.txt" => b"Hello, World!",
                }
            },
            "another.txt" => b"Another",
        };

        let timestamps = Some(Timestamps::default());

        let volume = v3_volume(dir);

        let volume = Volume::new(volume);

        let another_hash: [u8; 32] = sha2::Sha256::digest(b"Another").into();
        let file_hash: [u8; 32] = sha2::Sha256::digest(b"Hello, World!").into();
        let to_hash: [u8; 32] = sha2::Sha256::digest(&file_hash).into();
        let path_hash: [u8; 32] = sha2::Sha256::digest(&to_hash).into();

        assert!(volume.read_file("").is_none());
        assert_eq!(
            volume.read_file("/another.txt").unwrap(),
            (b"Another".as_slice().into(), Some(another_hash))
        );
        assert_eq!(
            volume.metadata("/another.txt").unwrap(),
            Metadata::File {
                length: 7,
                timestamps
            }
        );
        assert_eq!(
            volume.read_file("/path/to/file.txt").unwrap(),
            (b"Hello, World!".as_slice().into(), Some(file_hash)),
        );
        assert_eq!(
            volume.read_dir("/").unwrap(),
            vec![
                (
                    PathSegment::parse("another.txt").unwrap(),
                    Some(another_hash),
                    Metadata::File {
                        length: 7,
                        timestamps
                    },
                ),
                (
                    PathSegment::parse("path").unwrap(),
                    Some(path_hash),
                    Metadata::Dir { timestamps }
                ),
            ],
        );
        assert_eq!(
            volume.read_dir("/path").unwrap(),
            vec![(
                PathSegment::parse("to").unwrap(),
                Some(to_hash),
                Metadata::Dir { timestamps }
            )],
        );
        assert_eq!(
            volume.read_dir("/path/to/").unwrap(),
            vec![(
                PathSegment::parse("file.txt").unwrap(),
                Some(file_hash),
                Metadata::File {
                    length: 13,
                    timestamps
                }
            )],
        );
    }
}
