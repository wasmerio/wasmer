use std::{
    any::Any,
    borrow::Cow,
    collections::BTreeMap,
    fmt::Debug,
    fs::File,
    io::{BufRead, BufReader, Read, Seek},
    path::Path,
    str::FromStr,
    sync::Arc,
};

use bytes::{Buf, Bytes};
use sha2::Digest;
use shared_buffer::OwnedBuffer;

use webc::{PathSegmentError, Version};

use crate::package::{
    volume::abstract_volume::{Metadata, Volume},
    Package, WasmerPackageError,
};

/// A version-agnostic read-only WEBC container.
///
/// A `Container` provides a high-level interface for reading and manipulating
/// WEBC container files. It supports multiple versions of WEBC container
/// formats and abstracts the underlying differences between them.
#[derive(Debug, Clone)]
pub struct Container {
    imp: Arc<dyn AbstractWebc + Send + Sync>,
}

#[allow(clippy::result_large_err)]
impl Container {
    /// Load a [`Container`] from disk.
    ///
    /// Where possible, this will try to use a memory-mapped implementation
    /// to reduce memory usage.
    pub fn from_disk(path: impl AsRef<Path>) -> Result<Self, ContainerError> {
        let path = path.as_ref();

        if path.is_dir() {
            return parse_dir(path);
        }

        let mut f = File::open(path).map_err(|error| ContainerError::Open {
            error,
            path: path.to_path_buf(),
        })?;

        if is_tarball(&mut f) {
            return parse_tarball(BufReader::new(f));
        }

        match webc::detect(&mut f) {
            Ok(Version::V2) => parse_v2_mmap(f),
            Ok(Version::V3) => parse_v3_mmap(f),
            Ok(other) => {
                // fall back to the allocating generic version
                let mut buffer = Vec::new();
                f.rewind()
                    .and_then(|_| f.read_to_end(&mut buffer))
                    .map_err(|error| ContainerError::Read {
                        path: path.to_path_buf(),
                        error,
                    })?;

                Container::from_bytes_and_version(buffer.into(), other)
            }
            Err(e) => Err(ContainerError::Detect(e)),
        }
    }

    /// Load a [`Container`] from bytes in memory.
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<Self, ContainerError> {
        let bytes: Bytes = bytes.into();

        if is_tarball(std::io::Cursor::new(&bytes)) {
            return parse_tarball(bytes.reader());
        }

        let version = webc::detect(bytes.as_ref())?;
        Container::from_bytes_and_version(bytes, version)
    }

    /// Create a Container from an abstract implementation.
    #[doc(hidden)]
    pub fn new(repr: impl AbstractWebc + Send + Sync + 'static) -> Self {
        Container {
            imp: Arc::new(repr),
        }
    }

    fn from_bytes_and_version(bytes: Bytes, version: Version) -> Result<Self, ContainerError> {
        match version {
            Version::V2 => parse_v2_owned(bytes),
            Version::V3 => parse_v3_owned(bytes),
            other => Err(ContainerError::UnsupportedVersion(other)),
        }
    }

    /// Get the underlying webc version
    pub fn version(&self) -> Version {
        self.imp.version()
    }

    /// Get the [`Container`]'s manifest.
    pub fn manifest(&self) -> &webc::metadata::Manifest {
        self.imp.manifest()
    }

    /// Get the [`Container`]'s webc hash
    pub fn webc_hash(&self) -> Option<[u8; 32]> {
        self.imp.get_webc_hash()
    }

    /// Get all atoms stored in the container as a map.
    pub fn atoms(&self) -> BTreeMap<String, OwnedBuffer> {
        let mut atoms = BTreeMap::new();

        for name in self.imp.atom_names() {
            if let Some(atom) = self.imp.get_atom(&name) {
                atoms.insert(name.into_owned(), atom);
            }
        }

        atoms
    }

    /// Get an atom with the given name.
    ///
    /// Returns `None` if the atom does not exist in the container.
    ///
    /// This operation is pretty cheap, typically just a dictionary lookup
    /// followed by reference count bump and some index math.
    pub fn get_atom(&self, name: &str) -> Option<OwnedBuffer> {
        self.imp.get_atom(name)
    }

    /// Get all volumes stored in the container.
    pub fn volumes(&self) -> BTreeMap<String, Volume> {
        let mut volumes = BTreeMap::new();

        for name in self.imp.volume_names() {
            if let Some(atom) = self.imp.get_volume(&name) {
                volumes.insert(name.into_owned(), atom);
            }
        }

        volumes
    }

    /// Get a volume with the given name.
    ///
    /// Returns `None` if the volume does not exist in the container.
    pub fn get_volume(&self, name: &str) -> Option<Volume> {
        self.imp.get_volume(name)
    }

    /// Downcast the [`Container`] a concrete implementation.
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: 'static,
    {
        self.as_any().downcast_ref()
    }

    /// Downcast the [`Container`] a concrete implementation, returning the
    /// original [`Container`] if the cast fails.
    pub fn downcast<T>(self) -> Result<Arc<T>, Self>
    where
        T: 'static,
    {
        if self.as_any().is::<T>() {
            // Safety: We've just checked that the type matches up.
            unsafe { Ok(Arc::from_raw(Arc::into_raw(self.imp).cast())) }
        } else {
            Err(self)
        }
    }

    /// Unpack the container into a directory.
    ///
    /// This will create a directory at `out_dir` and populate it with the
    /// the contents of each volume and the manifest.
    ///
    /// If the output directory already exists and is not empty, the operation
    /// will fail, unless `overwrite` is set to `true`.
    pub fn unpack(&self, out_dir: &std::path::Path, overwrite: bool) -> Result<(), ContainerError> {
        match out_dir.metadata() {
            Ok(m) => {
                if !m.is_dir() {
                    return Err(ContainerError::Open {
                        path: out_dir.to_path_buf(),
                        error: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "output path is not a directory",
                        ),
                    });
                }
                let mut items = std::fs::read_dir(out_dir).map_err(|err| ContainerError::Open {
                    path: out_dir.to_path_buf(),
                    error: err,
                })?;

                if items.next().is_some() && !overwrite {
                    return Err(ContainerError::Open {
                        path: out_dir.to_path_buf(),
                        error: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "output directory is not empty",
                        ),
                    })?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(out_dir).map_err(|err| ContainerError::Open {
                    path: out_dir.to_path_buf(),
                    error: err,
                })?;
            }
            Err(err) => {
                return Err(ContainerError::Open {
                    path: out_dir.to_path_buf(),
                    error: err,
                });
            }
        };

        let manifest_path = out_dir.join("manifest.json");
        // NOTE: this serialization is infallible in practice, hence the unwrap.
        let manifest_data =
            serde_json::to_vec(self.manifest()).expect("could not serialize manifest to JSON");

        std::fs::write(&manifest_path, manifest_data).map_err(|err| ContainerError::Open {
            path: manifest_path,
            error: err,
        })?;

        for (root, volume) in self.volumes() {
            let root = root.strip_prefix('/').unwrap_or(root.as_str());

            let volume_dir = out_dir.join(root);

            volume.unpack("/", &volume_dir)?;
        }

        for (name, contents) in self.atoms() {
            std::fs::write(out_dir.join(name), contents)?;
        }

        Ok(())
    }

    /// Validates an [`AbstractWebc`]
    pub fn validate(&self) -> Result<(), anyhow::Error> {
        if self.version() == Version::V1 {
            anyhow::bail!("v1 validation is unsupported");
        }

        let manifest = self.manifest();

        // validate atoms
        for (name, bytes) in self.atoms().iter() {
            let signature = manifest.atom_signature(name)?;
            let expected = sha2::Sha256::digest(bytes);

            if signature.as_bytes() != expected.as_slice() {
                anyhow::bail!(format!(
                    "signature of atom: {name} does not match what is expected"
                ))
            }
        }

        if let Some(fs) = manifest.filesystem()? {
            // validate fs
            for webc::metadata::annotations::FileSystemMapping {
                volume_name,
                host_path,
                ..
            } in fs.iter()
            {
                // validate that volume exists
                let volume = self
                    .get_volume(volume_name)
                    .ok_or_else(|| anyhow::Error::msg(format!("could not find: {volume_name}")))?;

                // in v2, `host_path` should be accessible in the webc volume
                if self.version() == Version::V2 {
                    // host path must be present in v2
                    let host_path = host_path.clone().ok_or_else(|| {
                        anyhow::Error::msg("host_path is not present in fs mapping")
                    })?;
                    let host_path_segments = webc::PathSegments::from_str(&host_path)?;

                    volume.read_dir(host_path_segments).ok_or_else(|| {
                        anyhow::Error::msg(format!("could not read directory: {host_path}"))
                    })?;
                }
            }
        }

        for (_, volume) in self.volumes().iter() {
            traverse_volume(volume, webc::PathSegments::ROOT, self.version())?;
        }

        Ok(())
    }
}

fn traverse_volume(
    volume: &Volume,
    path: webc::PathSegments,
    version: webc::Version,
) -> Result<(), anyhow::Error> {
    let entries = volume
        .read_dir(&path)
        .ok_or_else(|| anyhow::Error::msg(format!("failed to read path: {path}")))?;

    for (name, read_dir_hash, metadata) in entries {
        let entry_path = path.join(name);
        match metadata {
            Metadata::Dir { .. } => traverse_volume(volume, entry_path, version)?,
            Metadata::File { length, .. } => {
                let (content, read_file_hash) =
                    volume.read_file(entry_path.clone()).ok_or_else(|| {
                        anyhow::Error::msg(format!("failed to read file: {entry_path}"))
                    })?;

                if content.len() != length {
                    anyhow::bail!("File: {entry_path} length does not match with the actual content: {} != {}", length, content.len());
                }

                // validate the hashes
                if version == webc::Version::V3 {
                    let expected: [u8; 32] = sha2::Sha256::digest(&content).into();

                    let read_dir_hash = read_dir_hash.ok_or_else(|| {
                        anyhow::Error::msg(format!(
                            "hash of {entry_path} is not present in V3 when calling read_dir"
                        ))
                    })?;

                    let read_file_hash = read_file_hash.ok_or_else(|| {
                        anyhow::Error::msg(format!(
                            "hash of {entry_path} is not present in V3 when calling read_file"
                        ))
                    })?;

                    if expected != read_dir_hash {
                        anyhow::bail!("hash of {entry_path} does not match the expected value when calling read_dir");
                    }

                    if expected != read_file_hash {
                        anyhow::bail!("hash of {entry_path} does not match the expected value when calling read_file");
                    }
                }
            }
        }
    }

    Ok(())
}

/// The AbstractWebc trait allows defining your own
/// Containers easily from memory
#[doc(hidden)]
pub trait AbstractWebc: AsAny + Debug {
    /// Returns the version of the webc container
    fn version(&self) -> Version;

    /// Get the [`Container`]'s manifest.
    fn manifest(&self) -> &webc::metadata::Manifest;

    /// Get all atom names stored in the container.
    fn atom_names(&self) -> Vec<Cow<'_, str>>;

    /// Get an atom.
    fn get_atom(&self, name: &str) -> Option<OwnedBuffer>;

    /// Get hash of the webc
    fn get_webc_hash(&self) -> Option<[u8; 32]>;

    /// Get atoms section hash
    fn get_atoms_hash(&self) -> Option<[u8; 32]>;

    /// Get all volumes names stored in the container.
    fn volume_names(&self) -> Vec<Cow<'_, str>>;

    /// Get the volume for a specific name.
    fn get_volume(&self, name: &str) -> Option<Volume>;
}

#[allow(clippy::result_large_err)]
fn parse_tarball(reader: impl BufRead) -> Result<Container, ContainerError> {
    let pkg = Package::from_tarball(reader).map_err(ContainerError::WasmerPackage)?;
    Ok(Container::new(pkg))
}

#[allow(clippy::result_large_err)]
fn parse_dir(path: &Path) -> Result<Container, ContainerError> {
    let wasmer_toml = path.join("wasmer.toml");
    let pkg = Package::from_manifest(wasmer_toml)?;
    Ok(Container::new(pkg))
}

#[allow(clippy::result_large_err)]
fn parse_v2_owned(bytes: Bytes) -> Result<Container, ContainerError> {
    let reader = webc::v2::read::OwnedReader::parse(bytes)?;
    Ok(Container::new(reader))
}

#[allow(clippy::result_large_err)]
fn parse_v2_mmap(f: File) -> Result<Container, ContainerError> {
    // Note: OwnedReader::from_file() will automatically try to
    // use a memory-mapped file when possible.
    let webc = webc::v2::read::OwnedReader::from_file(f)?;
    Ok(Container::new(webc))
}

#[allow(clippy::result_large_err)]
fn parse_v3_owned(bytes: Bytes) -> Result<Container, ContainerError> {
    let reader = webc::v3::read::OwnedReader::parse(bytes)?;
    Ok(Container::new(reader))
}

#[allow(clippy::result_large_err)]
fn parse_v3_mmap(f: File) -> Result<Container, ContainerError> {
    // Note: OwnedReader::from_file() will automatically try to
    // use a memory-mapped file when possible.
    let webc = webc::v3::read::OwnedReader::from_file(f)?;
    Ok(Container::new(webc))
}

impl AbstractWebc for webc::v2::read::OwnedReader {
    fn version(&self) -> Version {
        Version::V2
    }

    fn manifest(&self) -> &webc::metadata::Manifest {
        self.manifest()
    }

    fn atom_names(&self) -> Vec<Cow<'_, str>> {
        self.atom_names().map(Cow::Borrowed).collect()
    }

    fn get_atom(&self, name: &str) -> Option<OwnedBuffer> {
        self.get_atom(name).cloned().map(OwnedBuffer::from)
    }

    fn get_webc_hash(&self) -> Option<[u8; 32]> {
        self.webc_hash()
    }

    fn get_atoms_hash(&self) -> Option<[u8; 32]> {
        None
    }

    fn volume_names(&self) -> Vec<Cow<'_, str>> {
        webc::v2::read::OwnedReader::volume_names(self)
            .map(Cow::Borrowed)
            .collect()
    }

    fn get_volume(&self, name: &str) -> Option<Volume> {
        self.get_volume(name).ok().map(Volume::new)
    }
}

impl From<webc::v2::read::OwnedReader> for Container {
    fn from(value: webc::v2::read::OwnedReader) -> Self {
        Container::new(value)
    }
}

impl AbstractWebc for webc::v3::read::OwnedReader {
    fn version(&self) -> Version {
        Version::V3
    }

    fn manifest(&self) -> &webc::metadata::Manifest {
        self.manifest()
    }

    fn atom_names(&self) -> Vec<Cow<'_, str>> {
        self.atom_names().map(Cow::Borrowed).collect()
    }

    fn get_atom(&self, name: &str) -> Option<OwnedBuffer> {
        self.get_atom(name).cloned().map(|(_, b)| b)
    }

    fn get_webc_hash(&self) -> Option<[u8; 32]> {
        self.webc_hash()
    }

    fn get_atoms_hash(&self) -> Option<[u8; 32]> {
        Some(self.atoms_hash())
    }

    fn volume_names(&self) -> Vec<Cow<'_, str>> {
        webc::v3::read::OwnedReader::volume_names(self)
            .map(Cow::Borrowed)
            .collect()
    }

    fn get_volume(&self, name: &str) -> Option<Volume> {
        self.get_volume(name).ok().map(Volume::new)
    }
}

impl From<webc::v3::read::OwnedReader> for Container {
    fn from(value: webc::v3::read::OwnedReader) -> Self {
        Container::new(value)
    }
}

impl AbstractWebc for Package {
    fn version(&self) -> Version {
        Version::V3
    }

    fn manifest(&self) -> &webc::metadata::Manifest {
        self.manifest()
    }

    fn atom_names(&self) -> Vec<Cow<'_, str>> {
        self.atoms()
            .keys()
            .map(|s| Cow::Borrowed(s.as_str()))
            .collect()
    }

    fn get_atom(&self, name: &str) -> Option<OwnedBuffer> {
        self.atoms().get(name).cloned()
    }

    fn get_webc_hash(&self) -> Option<[u8; 32]> {
        self.webc_hash()
    }

    fn get_atoms_hash(&self) -> Option<[u8; 32]> {
        None
    }

    fn volume_names(&self) -> Vec<Cow<'_, str>> {
        self.volume_names()
    }

    fn get_volume(&self, name: &str) -> Option<Volume> {
        self.get_volume(name).map(Volume::new)
    }
}

impl From<Package> for Container {
    fn from(value: Package) -> Self {
        Container::new(value)
    }
}

/// A trait for downcasting a reference to a concrete type.
#[doc(hidden)]
pub trait AsAny {
    /// Downcast a reference to a concrete type.
    fn as_any(&self) -> &(dyn Any + 'static);
}

impl<T> AsAny for T
where
    T: Any,
{
    fn as_any(&self) -> &(dyn Any + 'static) {
        self
    }
}

/// Various errors that may occur during [`Container`] operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContainerError {
    /// Unable to detect the WEBC version.
    #[error("Unable to detect the WEBC version")]
    Detect(#[from] webc::DetectError),
    /// An unsupported WEBC version was found.
    #[error("Unsupported WEBC version, {_0}")]
    UnsupportedVersion(webc::Version),
    /// Parsing requires a feature to be enabled.
    #[error("Unable to parse because the \"{feature}\" must be enabled")]
    FeatureNotEnabled {
        /// The feature name
        feature: &'static str,
    },
    /// An error occurred while parsing a v2 WEBC file.
    #[error(transparent)]
    V2Owned(#[from] webc::v2::read::OwnedReaderError),
    /// An error occurred while parsing a v3 WEBC file.
    #[error(transparent)]
    V3Owned(#[from] webc::v3::read::OwnedReaderError),
    /// an error occurred while loading a Wasmer package from disk.
    #[error(transparent)]
    WasmerPackage(#[from] WasmerPackageError),
    /// Path segment parsing failed.
    #[error(transparent)]
    Path(#[from] PathSegmentError),
    /// Unable to open a file.
    #[error("Unable to open \"{}\"", path.display())]
    Open {
        /// The file's path.
        path: std::path::PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },
    /// Unable to read a file's contents into memory.
    #[error("Unable to read \"{}\"", path.display())]
    Read {
        /// The file's path.
        path: std::path::PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },
    /// An IO error
    #[error("IOError: {0:?}")]
    IOError(#[from] std::io::Error),
}

/// Check if something looks like a `*.tar.gz` file.
fn is_tarball(mut file: impl Read + Seek) -> bool {
    /// Magic bytes for a `*.tar.gz` file according to
    /// [Wikipedia](https://en.wikipedia.org/wiki/List_of_file_signatures).
    const TAR_GZ_MAGIC_BYTES: [u8; 2] = [0x1F, 0x8B];

    let mut buffer = [0_u8; 2];
    let result = match file.read_exact(&mut buffer) {
        Ok(_) => buffer == TAR_GZ_MAGIC_BYTES,
        Err(_) => false,
    };

    let _ = file.rewind();

    result
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{Container, Package};

    #[test]
    fn container_unpacks_atoms() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"

                [[module]]
                name = "foo"
                source = "foo.wasm"
                abi = "wasi"

                [fs]
                "/bar" = "bar"
            "#;

        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        let atom_path = temp.path().join("foo.wasm");
        std::fs::write(&atom_path, b"").unwrap();

        let bar = temp.path().join("bar");
        std::fs::create_dir(&bar).unwrap();

        let webc = Package::from_manifest(&manifest)
            .unwrap()
            .serialize()
            .unwrap();
        let container = Container::from_bytes(webc).unwrap();

        let out_dir = temp.path().join("out");
        container.unpack(&out_dir, false).unwrap();

        let expected_entries = vec![
            "bar",      // the volume
            "metadata", // the metadata volume
            "foo",      // the atom
            "manifest.json",
        ];
        let entries = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap())
            .collect::<Vec<_>>();

        assert_eq!(expected_entries.len(), entries.len());
        assert!(expected_entries.iter().all(|e| {
            entries
                .iter()
                .any(|entry| entry.file_name().as_os_str() == *e)
        }))
    }
}
