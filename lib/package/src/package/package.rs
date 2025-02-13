use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use shared_buffer::OwnedBuffer;
use tar::Archive;
use tempfile::TempDir;
use wasmer_config::package::Manifest as WasmerManifest;

use webc::{
    metadata::{annotations::Wapm, Manifest as WebcManifest},
    v3::{
        write::{FileEntry, Writer},
        ChecksumAlgorithm, Timestamps,
    },
    AbstractVolume, AbstractWebc, Container, ContainerError, DetectError, PathSegment, Version,
    Volume,
};

use super::{
    manifest::wasmer_manifest_to_webc,
    volume::{fs::FsVolume, WasmerPackageVolume},
    ManifestError, MemoryVolume, Strictness,
};

/// Errors that may occur while loading a Wasmer package from disk.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::result_large_err)]
#[non_exhaustive]
pub enum WasmerPackageError {
    /// Unable to create a temporary directory.
    #[error("Unable to create a temporary directory")]
    TempDir(#[source] std::io::Error),
    /// Unable to open a file.
    #[error("Unable to open \"{}\"", path.display())]
    FileOpen {
        /// The file being opened.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },
    /// Unable to read a file.
    #[error("Unable to read \"{}\"", path.display())]
    FileRead {
        /// The file being opened.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },

    /// Generic IO error.
    #[error("IO Error: {0:?}")]
    IoError(#[from] std::io::Error),

    /// Unexpected path format
    #[error("Malformed path format: {0:?}")]
    MalformedPath(PathBuf),

    /// Unable to extract the tarball.
    #[error("Unable to extract the tarball")]
    Tarball(#[source] std::io::Error),
    /// Unable to deserialize the `wasmer.toml` file.
    #[error("Unable to deserialize \"{}\"", path.display())]
    TomlDeserialize {
        /// The file being deserialized.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: toml::de::Error,
    },
    /// Unable to deserialize a json file.
    #[error("Unable to deserialize \"{}\"", path.display())]
    JsonDeserialize {
        /// The file being deserialized.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: serde_json::Error,
    },
    /// Unable to find the `wasmer.toml` file.
    #[error("Unable to find the \"wasmer.toml\"")]
    MissingManifest,
    /// Unable to canonicalize a path.
    #[error("Unable to get the absolute path for \"{}\"", path.display())]
    Canonicalize {
        /// The path being canonicalized.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },
    /// Unable to load the `wasmer.toml` manifest.
    #[error("Unable to load the \"wasmer.toml\" manifest")]
    Manifest(#[from] ManifestError),
    /// A manifest validation error.
    #[error("The manifest is invalid")]
    Validation(#[from] wasmer_config::package::ValidationError),
    /// A path in the fs mapping does not exist
    #[error("Path: \"{}\" does not exist", path.display())]
    PathNotExists {
        /// Path entry in fs mapping
        path: PathBuf,
    },
    /// Any error happening when populating the volumes tree map of a package
    #[error("Volume creation failed: {0:?}")]
    VolumeCreation(#[from] anyhow::Error),

    /// Error when serializing or deserializing
    #[error("serde error: {0:?}")]
    SerdeError(#[from] ciborium::value::Error),

    /// Container Error
    #[error("container error: {0:?}")]
    ContainerError(#[from] ContainerError),

    /// Detect Error
    #[error("detect error: {0:?}")]
    DetectError(#[from] DetectError),
}

/// A Wasmer package that will be lazily loaded from disk.
#[derive(Debug)]
pub struct Package {
    // base dir could be a temp dir, so we keep it around to prevent the directory
    // from being deleted
    #[allow(dead_code)]
    base_dir: BaseDir,
    manifest: WebcManifest,
    atoms: BTreeMap<String, OwnedBuffer>,
    strictness: Strictness,
    volumes: BTreeMap<String, Arc<dyn WasmerPackageVolume + Send + Sync + 'static>>,
}

impl Package {
    /// Load a [`Package`] from a `*.tar.gz` file on disk.
    ///
    /// # Implementation Details
    ///
    /// This will unpack the tarball to a temporary directory on disk and use
    /// memory-mapped files in order to reduce RAM usage.
    pub fn from_tarball_file(path: impl AsRef<Path>) -> Result<Self, WasmerPackageError> {
        Package::from_tarball_file_with_strictness(path.as_ref(), Strictness::default())
    }
    /// Load a [`Package`] from a `*.tar.gz` file on disk.
    ///
    /// # Implementation Details
    ///
    /// This will unpack the tarball to a temporary directory on disk and use
    /// memory-mapped files in order to reduce RAM usage.
    pub fn from_tarball_file_with_strictness(
        path: impl AsRef<Path>,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let path = path.as_ref();
        let f = File::open(path).map_err(|error| WasmerPackageError::FileOpen {
            path: path.to_path_buf(),
            error,
        })?;

        Package::from_tarball_with_strictness(BufReader::new(f), strictness)
    }

    /// Load a package from a `*.tar.gz` archive.
    pub fn from_tarball(tarball: impl BufRead) -> Result<Self, WasmerPackageError> {
        Package::from_tarball_with_strictness(tarball, Strictness::default())
    }

    /// Load a package from a `*.tar.gz` archive.
    pub fn from_tarball_with_strictness(
        tarball: impl BufRead,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let tarball = GzDecoder::new(tarball);
        let temp = tempdir().map_err(WasmerPackageError::TempDir)?;
        let archive = Archive::new(tarball);
        unpack_archive(archive, temp.path()).map_err(WasmerPackageError::Tarball)?;

        let (_manifest_path, manifest) = read_manifest(temp.path())?;

        Package::load(manifest, temp, strictness)
    }

    /// Load a package from a `wasmer.toml` manifest on disk.
    pub fn from_manifest(wasmer_toml: impl AsRef<Path>) -> Result<Self, WasmerPackageError> {
        Package::from_manifest_with_strictness(wasmer_toml, Strictness::default())
    }

    /// Load a package from a `wasmer.toml` manifest on disk.
    pub fn from_manifest_with_strictness(
        wasmer_toml: impl AsRef<Path>,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let path = wasmer_toml.as_ref();
        let path = path
            .canonicalize()
            .map_err(|error| WasmerPackageError::Canonicalize {
                path: path.to_path_buf(),
                error,
            })?;

        let wasmer_toml =
            std::fs::read_to_string(&path).map_err(|error| WasmerPackageError::FileRead {
                path: path.to_path_buf(),
                error,
            })?;
        let wasmer_toml: WasmerManifest =
            toml::from_str(&wasmer_toml).map_err(|error| WasmerPackageError::TomlDeserialize {
                path: path.to_path_buf(),
                error,
            })?;

        let base_dir = path
            .parent()
            .expect("Canonicalizing should always result in a file with a parent directory")
            .to_path_buf();

        for path in wasmer_toml.fs.values() {
            if !base_dir.join(path).exists() {
                return Err(WasmerPackageError::PathNotExists { path: path.clone() });
            }
        }

        Package::load(wasmer_toml, base_dir, strictness)
    }

    /// (Re)loads a package from a manifest.json file which was created as the result of calling [`Container::unpack`](crate::Container::unpack)
    pub fn from_json_manifest(manifest: PathBuf) -> Result<Self, WasmerPackageError> {
        Self::from_json_manifest_with_strictness(manifest, Strictness::default())
    }

    /// (Re)loads a package from a manifest.json file which was created as the result of calling [`Container::unpack`](crate::Container::unpack)
    pub fn from_json_manifest_with_strictness(
        manifest: PathBuf,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let base_dir = manifest
            .parent()
            .expect("Canonicalizing should always result in a file with a parent directory")
            .to_path_buf();

        let base_dir: BaseDir = base_dir.into();

        let contents = std::fs::read(&manifest)?;
        let manifest: WebcManifest =
            serde_json::from_slice(&contents).map_err(|e| WasmerPackageError::JsonDeserialize {
                path: manifest.clone(),
                error: e,
            })?;

        let mut atoms = BTreeMap::<String, OwnedBuffer>::new();
        for atom in manifest.atoms.keys() {
            let path = base_dir.path().join(atom);

            let contents = std::fs::read(&path)
                .map_err(|e| WasmerPackageError::FileRead { path, error: e })?;

            atoms.insert(atom.clone(), contents.into());
        }

        let mut volumes: BTreeMap<String, Arc<dyn WasmerPackageVolume + Send + Sync + 'static>> =
            BTreeMap::new();
        if let Some(fs_mappings) = manifest.filesystem()? {
            for entry in fs_mappings.iter() {
                let mut dirs = BTreeSet::new();
                let path = entry.volume_name.strip_prefix('/').ok_or_else(|| {
                    WasmerPackageError::MalformedPath(PathBuf::from(&entry.volume_name))
                })?;
                let path = base_dir.path().join(path);
                dirs.insert(path);

                volumes.insert(
                    entry.volume_name.clone(),
                    Arc::new(FsVolume::new(
                        entry.volume_name.clone(),
                        base_dir.path().to_owned(),
                        BTreeSet::new(),
                        dirs,
                    )),
                );
            }
        }

        let mut files = BTreeSet::new();
        for entry in std::fs::read_dir(base_dir.path().join(FsVolume::METADATA))? {
            let entry = entry?;

            files.insert(entry.path());
        }

        if let Some(wapm) = manifest.wapm().unwrap() {
            if let Some(license_file) = wapm.license_file.as_ref() {
                let path = license_file.path.strip_prefix('/').ok_or_else(|| {
                    WasmerPackageError::MalformedPath(PathBuf::from(&license_file.path))
                })?;
                let path = base_dir.path().join(FsVolume::METADATA).join(path);

                files.insert(path);
            }

            if let Some(readme_file) = wapm.readme.as_ref() {
                let path = readme_file.path.strip_prefix('/').ok_or_else(|| {
                    WasmerPackageError::MalformedPath(PathBuf::from(&readme_file.path))
                })?;
                let path = base_dir.path().join(FsVolume::METADATA).join(path);

                files.insert(path);
            }
        }

        volumes.insert(
            FsVolume::METADATA.to_string(),
            Arc::new(FsVolume::new_with_intermediate_dirs(
                FsVolume::METADATA.to_string(),
                base_dir.path().join(FsVolume::METADATA).to_owned(),
                files,
                BTreeSet::new(),
            )),
        );

        Ok(Package {
            base_dir,
            manifest,
            atoms,
            strictness,
            volumes,
        })
    }

    /// Create a [`Package`] from an in-memory representation.
    pub fn from_in_memory(
        manifest: WasmerManifest,
        volumes: BTreeMap<String, MemoryVolume>,
        atoms: BTreeMap<String, (Option<String>, OwnedBuffer)>,
        metadata: MemoryVolume,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let mut new_volumes = BTreeMap::new();

        for (k, v) in volumes.into_iter() {
            new_volumes.insert(k, Arc::new(v) as _);
        }

        new_volumes.insert(MemoryVolume::METADATA.to_string(), Arc::new(metadata) as _);

        let volumes = new_volumes;

        let (mut manifest, atoms) =
            super::manifest::in_memory_wasmer_manifest_to_webc(&manifest, &atoms)?;

        if let Some(entry) = manifest.package.get_mut(Wapm::KEY) {
            let mut wapm: Wapm = entry.deserialized()?;

            wapm.name.take();
            wapm.version.take();
            wapm.description.take();

            *entry = ciborium::value::Value::serialized(&wapm)?;
        };

        Ok(Package {
            base_dir: BaseDir::Path(Path::new("/").to_path_buf()),
            manifest,
            atoms,
            strictness,
            volumes,
        })
    }

    fn load(
        wasmer_toml: WasmerManifest,
        base_dir: impl Into<BaseDir>,
        strictness: Strictness,
    ) -> Result<Self, WasmerPackageError> {
        let base_dir = base_dir.into();

        if strictness.is_strict() {
            wasmer_toml.validate()?;
        }

        let (mut manifest, atoms) =
            wasmer_manifest_to_webc(&wasmer_toml, base_dir.path(), strictness)?;

        // remove name, description, and version before creating the webc file
        if let Some(entry) = manifest.package.get_mut(Wapm::KEY) {
            let mut wapm: Wapm = entry.deserialized()?;

            wapm.name.take();
            wapm.version.take();
            wapm.description.take();

            *entry = ciborium::value::Value::serialized(&wapm)?;
        };

        // Create volumes
        let base_dir_path = base_dir.path().to_path_buf();
        // Create metadata volume
        let metadata_volume = FsVolume::new_metadata(&wasmer_toml, base_dir_path.clone())?;
        // Create assets volume
        let mut volumes: BTreeMap<String, Arc<dyn WasmerPackageVolume + Send + Sync + 'static>> = {
            let old = FsVolume::new_assets(&wasmer_toml, &base_dir_path)?;
            let mut new = BTreeMap::new();

            for (k, v) in old.into_iter() {
                new.insert(k, Arc::new(v) as _);
            }

            new
        };
        volumes.insert(
            metadata_volume.name().to_string(),
            Arc::new(metadata_volume),
        );

        Ok(Package {
            base_dir,
            manifest,
            atoms,
            strictness,
            volumes,
        })
    }

    /// Returns the Sha256 has of the webc represented by this Package
    pub fn webc_hash(&self) -> Option<[u8; 32]> {
        None
    }

    /// Get the WEBC manifest.
    pub fn manifest(&self) -> &WebcManifest {
        &self.manifest
    }

    /// Get all atoms in this package.
    pub fn atoms(&self) -> &BTreeMap<String, OwnedBuffer> {
        &self.atoms
    }

    /// Returns all volumes in this package
    pub fn volumes(
        &self,
    ) -> impl Iterator<Item = &Arc<dyn WasmerPackageVolume + Sync + Send + 'static>> {
        self.volumes.values()
    }

    /// Serialize the package to a `*.webc` v2 file, ignoring errors due to
    /// missing files.
    pub fn serialize(&self) -> Result<Bytes, Error> {
        let mut w = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(self.manifest())?
            .write_atoms(self.atom_entries()?)?;

        for (name, volume) in &self.volumes {
            w.write_volume(name.as_str(), volume.as_directory_tree(self.strictness)?)?;
        }

        let serialized = w.finish(webc::v3::SignatureAlgorithm::None)?;

        Ok(serialized)
    }

    fn atom_entries(&self) -> Result<BTreeMap<PathSegment, FileEntry<'_>>, Error> {
        self.atoms()
            .iter()
            .map(|(key, value)| {
                let filename = PathSegment::parse(key)
                    .with_context(|| format!("\"{key}\" isn't a valid atom name"))?;
                // FIXME: maybe?
                Ok((filename, FileEntry::borrowed(value, Timestamps::default())))
            })
            .collect()
    }

    pub(crate) fn get_volume(
        &self,
        name: &str,
    ) -> Option<Arc<dyn WasmerPackageVolume + Sync + Send + 'static>> {
        self.volumes.get(name).cloned()
    }

    pub(crate) fn volume_names(&self) -> Vec<Cow<'_, str>> {
        self.volumes
            .keys()
            .map(|name| Cow::Borrowed(name.as_str()))
            .collect()
    }
}

impl AbstractWebc for Package {
    fn version(&self) -> Version {
        Version::V3
    }

    fn manifest(&self) -> &WebcManifest {
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
        self.get_volume(name).map(|v| {
            let a: Arc<dyn AbstractVolume + Send + Sync + 'static> = v.as_volume();

            Volume::from(a)
        })
    }
}

impl From<Package> for Container {
    fn from(value: Package) -> Self {
        Container::new(value)
    }
}

const IS_WASI: bool = cfg!(all(target_family = "wasm", target_os = "wasi"));

/// A polyfill for [`TempDir::new()`] that will work when compiling to
/// WASI-based targets.
///
/// This works around [`std::env::temp_dir()`][tempdir] panicking
/// unconditionally on WASI.
///
/// [tempdir]: https://github.com/wasix-org/rust/blob/ef19cdcdff77047f1e5ea4d09b4869d6fa456cc7/library/std/src/sys/wasi/os.rs#L228-L230
fn tempdir() -> Result<TempDir, std::io::Error> {
    if !IS_WASI {
        // The happy path.
        return TempDir::new();
    }

    // Note: When compiling to wasm32-wasip1, we can't use TempDir::new()
    // because std::env::temp_dir() will unconditionally panic.
    let temp_dir: PathBuf = std::env::var("TMPDIR")
        .unwrap_or_else(|_| "/tmp".to_string())
        .into();

    if temp_dir.exists() {
        TempDir::new_in(temp_dir)
    } else {
        // The temporary directory doesn't exist. A naive create_dir_all()
        // doesn't work when running with "wasmer run" because the root
        // directory is immutable, so let's try to use the current exe's
        // directory as our tempdir.
        // See also: https://github.com/wasmerio/wasmer/blob/482b78890b789f6867a91be9f306385e6255b260/lib/wasix/src/syscalls/wasi/path_create_directory.rs#L30-L32
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                if let Ok(temp) = TempDir::new_in(parent) {
                    return Ok(temp);
                }
            }
        }

        // Oh well, this will probably fail, but at least we tried.
        std::fs::create_dir_all(&temp_dir)?;
        TempDir::new_in(temp_dir)
    }
}

/// A polyfill for [`Archive::unpack()`] that is WASI-compatible.
///
/// This works around `canonicalize()` being [unsupported][github] on
/// `wasm32-wasip1`.
///
/// [github]: https://github.com/rust-lang/rust/blob/5b1dc9de77106cb08ce9a1a8deaa14f52751d7e4/library/std/src/sys/wasi/fs.rs#L654-L658
fn unpack_archive(
    mut archive: Archive<impl std::io::Read>,
    dest: &Path,
) -> Result<(), std::io::Error> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_family = "wasm", target_os = "wasi"))]
        {
            // A naive version of unpack() that should be good enough for WASI
            // https://github.com/alexcrichton/tar-rs/blob/c77f47cb1b4b47fc4404a170d9d91cb42cc762ea/src/archive.rs#L216-L247
            for entry in archive.entries()? {
                let mut entry = entry?;
                let item_path = entry.path()?;
                let full_path = resolve_archive_path(dest, &item_path);

                match entry.header().entry_type() {
                    tar::EntryType::Directory => {
                        std::fs::create_dir_all(&full_path)?;
                    }
                    tar::EntryType::Regular => {
                        if let Some(parent) = full_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut f = File::create(&full_path)?;
                        std::io::copy(&mut entry, &mut f)?;

                        let mtime = entry.header().mtime().unwrap_or_default();
                        if let Err(e) = set_timestamp(full_path.as_path(), mtime) {
                            println!("WARN: {e:?}");
                        }
                    }
                    _ => {}
                }
            }
            Ok(())

        } else {
            archive.unpack(dest)
        }
    }
}

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
fn set_timestamp(path: &Path, timestamp: u64) -> Result<(), anyhow::Error> {
    let fd = unsafe {
        libc::open(
            path.as_os_str().as_encoded_bytes().as_ptr() as _,
            libc::O_RDONLY,
        )
    };

    if fd < 0 {
        anyhow::bail!(format!("failed to open: {}", path.display()));
    }

    let timespec = [
        // accessed
        libc::timespec {
            tv_sec: unsafe { libc::time(std::ptr::null_mut()) }, // now
            tv_nsec: 0,
        },
        // modified
        libc::timespec {
            tv_sec: timestamp as i64,
            tv_nsec: 0,
        },
    ];

    let res = unsafe { libc::futimens(fd, timespec.as_ptr() as _) };

    if res < 0 {
        anyhow::bail!("failed to set timestamp for: {}", path.display());
    }

    Ok(())
}

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
fn resolve_archive_path(base_dir: &Path, path: &Path) -> PathBuf {
    let mut buffer = base_dir.to_path_buf();

    for component in path.components() {
        match component {
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                buffer.pop();
            }
            std::path::Component::Normal(segment) => {
                buffer.push(segment);
            }
        }
    }

    buffer
}

fn read_manifest(base_dir: &Path) -> Result<(PathBuf, WasmerManifest), WasmerPackageError> {
    for path in ["wasmer.toml", "wapm.toml"] {
        let path = base_dir.join(path);

        match std::fs::read_to_string(&path) {
            Ok(s) => {
                let toml_file = toml::from_str(&s).map_err({
                    let path = path.clone();
                    |error| WasmerPackageError::TomlDeserialize { path, error }
                })?;

                return Ok((path, toml_file));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(WasmerPackageError::FileRead { path, error });
            }
        }
    }

    Err(WasmerPackageError::MissingManifest)
}

#[derive(Debug)]
enum BaseDir {
    /// An existing directory.
    Path(PathBuf),
    /// A temporary directory that will be deleted on drop.
    Temp(TempDir),
}

impl BaseDir {
    fn path(&self) -> &Path {
        match self {
            BaseDir::Path(p) => p.as_path(),
            BaseDir::Temp(t) => t.path(),
        }
    }
}

impl From<TempDir> for BaseDir {
    fn from(v: TempDir) -> Self {
        Self::Temp(v)
    }
}

impl From<PathBuf> for BaseDir {
    fn from(v: PathBuf) -> Self {
        Self::Path(v)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs::File,
        path::{Path, PathBuf},
        str::FromStr,
        time::SystemTime,
    };

    use flate2::{write::GzEncoder, Compression};
    use sha2::Digest;
    use shared_buffer::OwnedBuffer;
    use tempfile::TempDir;
    use webc::{
        metadata::{
            annotations::{FileSystemMapping, VolumeSpecificPath},
            Binding, BindingsExtended, WaiBindings, WitBindings,
        },
        PathSegment, PathSegments,
    };

    use crate::{package::*, utils::from_bytes};

    #[test]
    fn nonexistent_files() {
        let temp = TempDir::new().unwrap();

        assert!(Package::from_manifest(temp.path().join("nonexistent.toml")).is_err());
        assert!(Package::from_tarball_file(temp.path().join("nonexistent.tar.gz")).is_err());
    }

    #[test]
    fn load_a_tarball() {
        let coreutils = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("old-tar-gz")
            .join("coreutils-1.0.11.tar.gz");
        assert!(coreutils.exists());

        let package = Package::from_tarball_file(coreutils).unwrap();

        let wapm = package.manifest().wapm().unwrap().unwrap();
        assert!(wapm.name.is_none());
        assert!(wapm.version.is_none());
        assert!(wapm.description.is_none());
    }

    #[test]
    fn tarball_with_no_manifest() {
        let temp = TempDir::new().unwrap();
        let empty_tarball = temp.path().join("empty.tar.gz");
        let mut f = File::create(&empty_tarball).unwrap();
        tar::Builder::new(GzEncoder::new(&mut f, Compression::fast()))
            .finish()
            .unwrap();

        assert!(Package::from_tarball_file(&empty_tarball).is_err());
    }

    #[test]
    fn empty_package_on_disk() {
        let temp = TempDir::new().unwrap();
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(
            &manifest,
            r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "A dummy package"
            "#,
        )
        .unwrap();

        let package = Package::from_manifest(&manifest).unwrap();

        let wapm = package.manifest().wapm().unwrap().unwrap();
        assert!(wapm.name.is_none());
        assert!(wapm.version.is_none());
        assert!(wapm.description.is_none());
    }

    #[test]
    fn load_old_cowsay() {
        let tarball = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("old-tar-gz")
            .join("cowsay-0.3.0.tar.gz");

        let pkg = Package::from_tarball_file(tarball).unwrap();

        insta::assert_yaml_snapshot!(pkg.manifest());
        assert_eq!(
            pkg.manifest.commands.keys().collect::<Vec<_>>(),
            ["cowsay", "cowthink"],
        );
    }

    #[test]
    fn serialize_package_with_non_existent_fs() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"

                [fs]
                "/first" = "./first"
            "#;
        let manifest = temp.path().join("wasmer.toml");

        std::fs::write(&manifest, wasmer_toml).unwrap();

        let error = Package::from_manifest(manifest).unwrap_err();

        match error {
            WasmerPackageError::PathNotExists { path } => {
                assert_eq!(path, PathBuf::from_str("./first").unwrap());
            }
            e => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn serialize_package_with_bundled_directories() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"

                [fs]
                "/first" = "first"
                second = "nested/dir"
                "second/child" = "third"
                empty = "empty"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        // Now we want to set up the following filesystem tree:
        //
        // - first/ ("/first")
        //   - file.txt
        // - nested/
        //   - dir/ ("second")
        //     - .wasmerignore
        //     - .hidden (should be ignored)
        //     - ignore_me (should be ignored)
        //     - README.md
        //     - another-dir/
        //       - empty.txt
        // - third/ ("second/child")
        //   - file.txt
        // - empty/ ("empty")
        //
        // The "/first" entry
        let first = temp.path().join("first");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(first.join("file.txt"), "File").unwrap();
        // The "second" entry
        let second = temp.path().join("nested").join("dir");
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(second.join(".wasmerignore"), "ignore_me").unwrap();
        std::fs::write(second.join(".hidden"), "something something").unwrap();
        std::fs::write(second.join("ignore_me"), "something something").unwrap();
        std::fs::write(second.join("README.md"), "please").unwrap();
        let another_dir = temp.path().join("nested").join("dir").join("another-dir");
        std::fs::create_dir_all(&another_dir).unwrap();
        std::fs::write(another_dir.join("empty.txt"), "").unwrap();
        // The "second/child" entry
        let third = temp.path().join("third");
        std::fs::create_dir_all(&third).unwrap();
        std::fs::write(third.join("file.txt"), "Hello, World!").unwrap();
        // The "empty" entry
        let empty_dir = temp.path().join("empty");
        std::fs::create_dir_all(empty_dir).unwrap();

        let package = Package::from_manifest(manifest).unwrap();

        let webc = package.serialize().unwrap();
        let webc = from_bytes(webc).unwrap();
        let manifest = webc.manifest();
        let wapm_metadata = manifest.wapm().unwrap().unwrap();
        assert!(wapm_metadata.name.is_none());
        assert!(wapm_metadata.version.is_none());
        assert!(wapm_metadata.description.is_none());
        let fs_table = manifest.filesystem().unwrap().unwrap();
        assert_eq!(
            fs_table,
            [
                FileSystemMapping {
                    from: None,
                    volume_name: "/first".to_string(),
                    host_path: None,
                    mount_path: "/first".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "/nested/dir".to_string(),
                    host_path: None,
                    mount_path: "/second".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "/third".to_string(),
                    host_path: None,
                    mount_path: "/second/child".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "/empty".to_string(),
                    host_path: None,
                    mount_path: "/empty".to_string(),
                },
            ]
        );

        let first_file_hash: [u8; 32] = sha2::Sha256::digest(b"File").into();
        let readme_hash: [u8; 32] = sha2::Sha256::digest(b"please").into();
        let empty_hash: [u8; 32] = sha2::Sha256::digest(b"").into();
        let third_file_hash: [u8; 32] = sha2::Sha256::digest(b"Hello, World!").into();

        let first_volume = webc.get_volume("/first").unwrap();
        assert_eq!(
            first_volume.read_file("/file.txt").unwrap(),
            (b"File".as_slice().into(), Some(first_file_hash)),
        );

        let nested_dir_volume = webc.get_volume("/nested/dir").unwrap();
        assert_eq!(
            nested_dir_volume.read_file("README.md").unwrap(),
            (b"please".as_slice().into(), Some(readme_hash)),
        );
        assert!(nested_dir_volume.read_file(".wasmerignore").is_none());
        assert!(nested_dir_volume.read_file(".hidden").is_none());
        assert!(nested_dir_volume.read_file("ignore_me").is_none());
        assert_eq!(
            nested_dir_volume
                .read_file("/another-dir/empty.txt")
                .unwrap(),
            (b"".as_slice().into(), Some(empty_hash))
        );

        let third_volume = webc.get_volume("/third").unwrap();
        assert_eq!(
            third_volume.read_file("/file.txt").unwrap(),
            (b"Hello, World!".as_slice().into(), Some(third_file_hash))
        );

        let empty_volume = webc.get_volume("/empty").unwrap();
        assert_eq!(
            empty_volume.read_dir("/").unwrap().len(),
            0,
            "Directories should be included, even if empty"
        );
    }

    #[test]
    fn serialize_package_with_metadata_files() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"
                readme = "README.md"
                license-file = "LICENSE"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();

        let serialized = Package::from_manifest(manifest)
            .unwrap()
            .serialize()
            .unwrap();

        let webc = from_bytes(serialized).unwrap();
        let metadata_volume = webc.get_volume("metadata").unwrap();

        let readme_hash: [u8; 32] = sha2::Sha256::digest(b"readme").into();
        let license_hash: [u8; 32] = sha2::Sha256::digest(b"license").into();

        assert_eq!(
            metadata_volume.read_file("/README.md").unwrap(),
            (b"readme".as_slice().into(), Some(readme_hash))
        );
        assert_eq!(
            metadata_volume.read_file("/LICENSE").unwrap(),
            (b"license".as_slice().into(), Some(license_hash))
        );
    }

    #[test]
    fn load_package_with_wit_bindings() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""

            [[module]]
            name = "my-lib"
            source = "./my-lib.wasm"
            abi = "none"
            bindings = { wit-bindgen = "0.1.0", wit-exports = "./file.wit" }
        "#;
        std::fs::write(temp.path().join("wasmer.toml"), wasmer_toml).unwrap();
        std::fs::write(temp.path().join("file.wit"), "file").unwrap();
        std::fs::write(temp.path().join("my-lib.wasm"), b"\0asm...").unwrap();

        let package = Package::from_manifest(temp.path().join("wasmer.toml"))
            .unwrap()
            .serialize()
            .unwrap();
        let webc = from_bytes(package).unwrap();

        assert_eq!(
            webc.manifest().bindings,
            vec![Binding {
                name: "library-bindings".to_string(),
                kind: "wit@0.1.0".to_string(),
                annotations: ciborium::value::Value::serialized(&BindingsExtended::Wit(
                    WitBindings {
                        exports: "metadata://file.wit".to_string(),
                        module: "my-lib".to_string(),
                    }
                ))
                .unwrap(),
            }]
        );
        let metadata_volume = webc.get_volume("metadata").unwrap();
        let file_hash: [u8; 32] = sha2::Sha256::digest(b"file").into();
        assert_eq!(
            metadata_volume.read_file("/file.wit").unwrap(),
            (b"file".as_slice().into(), Some(file_hash))
        );
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(webc.manifest()); }
        }
    }

    #[test]
    fn load_package_with_wai_bindings() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""

            [[module]]
            name = "my-lib"
            source = "./my-lib.wasm"
            abi = "none"
            bindings = { wai-version = "0.2.0", exports = "./file.wai", imports = ["a.wai", "b.wai"] }
        "#;
        std::fs::write(temp.path().join("wasmer.toml"), wasmer_toml).unwrap();
        std::fs::write(temp.path().join("file.wai"), "file").unwrap();
        std::fs::write(temp.path().join("a.wai"), "a").unwrap();
        std::fs::write(temp.path().join("b.wai"), "b").unwrap();
        std::fs::write(temp.path().join("my-lib.wasm"), b"\0asm...").unwrap();

        let package = Package::from_manifest(temp.path().join("wasmer.toml"))
            .unwrap()
            .serialize()
            .unwrap();
        let webc = from_bytes(package).unwrap();

        assert_eq!(
            webc.manifest().bindings,
            vec![Binding {
                name: "library-bindings".to_string(),
                kind: "wai@0.2.0".to_string(),
                annotations: ciborium::value::Value::serialized(&BindingsExtended::Wai(
                    WaiBindings {
                        exports: Some("metadata://file.wai".to_string()),
                        module: "my-lib".to_string(),
                        imports: vec![
                            "metadata://a.wai".to_string(),
                            "metadata://b.wai".to_string(),
                        ]
                    }
                ))
                .unwrap(),
            }]
        );
        let metadata_volume = webc.get_volume("metadata").unwrap();

        let file_hash: [u8; 32] = sha2::Sha256::digest(b"file").into();
        let a_hash: [u8; 32] = sha2::Sha256::digest(b"a").into();
        let b_hash: [u8; 32] = sha2::Sha256::digest(b"b").into();

        assert_eq!(
            metadata_volume.read_file("/file.wai").unwrap(),
            (b"file".as_slice().into(), Some(file_hash))
        );
        assert_eq!(
            metadata_volume.read_file("/a.wai").unwrap(),
            (b"a".as_slice().into(), Some(a_hash))
        );
        assert_eq!(
            metadata_volume.read_file("/b.wai").unwrap(),
            (b"b".as_slice().into(), Some(b_hash))
        );
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(webc.manifest()); }
        }
    }

    /// See <https://github.com/wasmerio/pirita/issues/105> for more.
    #[test]
    fn absolute_paths_in_wasmer_toml_issue_105() {
        let temp = TempDir::new().unwrap();
        let base_dir = temp.path().canonicalize().unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        let wasmer_toml = format!(
            r#"
                [package]
                name = 'some/package'
                version = '0.0.0'
                description = 'Test package'
                readme = '{BASE_DIR}{sep}README.md'
                license-file = '{BASE_DIR}{sep}LICENSE'

                [[module]]
                name = 'first'
                source = '{BASE_DIR}{sep}target{sep}debug{sep}package.wasm'
                bindings = {{ wai-version = '0.2.0', exports = '{BASE_DIR}{sep}bindings{sep}file.wai', imports = ['{BASE_DIR}{sep}bindings{sep}a.wai'] }}
            "#,
            BASE_DIR = base_dir.display(),
        );
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, &wasmer_toml).unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();
        let bindings = temp.path().join("bindings");
        std::fs::create_dir_all(&bindings).unwrap();
        std::fs::write(bindings.join("file.wai"), "file.wai").unwrap();
        std::fs::write(bindings.join("a.wai"), "a.wai").unwrap();
        let target = temp.path().join("target").join("debug");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("package.wasm"), b"\0asm...").unwrap();

        let serialized = Package::from_manifest(manifest)
            .unwrap()
            .serialize()
            .unwrap();

        let webc = from_bytes(serialized).unwrap();
        let manifest = webc.manifest();
        let wapm = manifest.wapm().unwrap().unwrap();

        // we should be able to look up the files using the manifest
        let lookup = |item: VolumeSpecificPath| {
            let volume = webc.get_volume(&item.volume).unwrap();
            let (contents, _) = volume.read_file(&item.path).unwrap();
            String::from_utf8(contents.into()).unwrap()
        };
        assert_eq!(lookup(wapm.license_file.unwrap()), "license");
        assert_eq!(lookup(wapm.readme.unwrap()), "readme");

        // The paths for bindings are stored slightly differently, but it's the
        // same general idea
        let lookup = |item: &str| {
            let (volume, path) = item.split_once(":/").unwrap();
            let volume = webc.get_volume(volume).unwrap();
            let (content, _) = volume.read_file(path).unwrap();
            String::from_utf8(content.into()).unwrap()
        };
        let bindings = manifest.bindings[0].get_wai_bindings().unwrap();
        assert_eq!(lookup(&bindings.imports[0]), "a.wai");
        assert_eq!(lookup(bindings.exports.unwrap().as_str()), "file.wai");

        // Snapshot tests for good measure
        let mut settings = insta::Settings::clone_current();
        let base_dir = base_dir.display().to_string();
        settings.set_description(wasmer_toml.replace(&base_dir, "[BASE_DIR]"));
        let filter = regex::escape(&base_dir);
        settings.add_filter(&filter, "[BASE_DIR]");
        settings.bind(|| {
            insta::assert_yaml_snapshot!(webc.manifest());
        });
    }

    #[test]
    fn serializing_will_skip_missing_metadata_by_default() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = 'some/package'
                version = '0.0.0'
                description = 'Test package'
                readme = '/this/does/not/exist/README.md'
                license-file = 'LICENSE.wtf'
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        let pkg = Package::from_manifest(manifest).unwrap();

        let serialized = pkg.serialize().unwrap();

        let webc = from_bytes(serialized).unwrap();
        let manifest = webc.manifest();
        let wapm = manifest.wapm().unwrap().unwrap();
        // We re-wrote the WAPM annotations to just not include the license file
        assert!(wapm.license_file.is_none());
        assert!(wapm.readme.is_none());

        // Note: serializing in strict mode should still fail
        let pkg = Package {
            strictness: Strictness::Strict,
            ..pkg
        };
        assert!(pkg.serialize().is_err());
    }

    #[test]
    fn serialize_package_without_local_base_fs_paths() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"
                readme = 'README.md'
                license-file = 'LICENSE'

                [fs]
                "/path_in_wasix" = "local-dir/dir1"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();

        // Now we want to set up the following filesystem tree:
        //
        // - local-dir/
        //   - dir1/
        //     - a
        //     - b
        let dir1 = temp.path().join("local-dir").join("dir1");
        std::fs::create_dir_all(&dir1).unwrap();

        let a = dir1.join("a");
        let b = dir1.join("b");

        std::fs::write(a, "a").unwrap();
        std::fs::write(b, "b").unwrap();

        let package = Package::from_manifest(manifest).unwrap();

        let webc = package.serialize().unwrap();
        let webc = from_bytes(webc).unwrap();
        let manifest = webc.manifest();
        let wapm_metadata = manifest.wapm().unwrap().unwrap();

        assert!(wapm_metadata.name.is_none());
        assert!(wapm_metadata.version.is_none());
        assert!(wapm_metadata.description.is_none());

        let fs_table = manifest.filesystem().unwrap().unwrap();
        assert_eq!(
            fs_table,
            [FileSystemMapping {
                from: None,
                volume_name: "/local-dir/dir1".to_string(),
                host_path: None,
                mount_path: "/path_in_wasix".to_string(),
            },]
        );

        let readme_hash: [u8; 32] = sha2::Sha256::digest(b"readme").into();
        let license_hash: [u8; 32] = sha2::Sha256::digest(b"license").into();

        let a_hash: [u8; 32] = sha2::Sha256::digest(b"a").into();
        let b_hash: [u8; 32] = sha2::Sha256::digest(b"b").into();

        let dir1_volume = webc.get_volume("/local-dir/dir1").unwrap();
        let meta_volume = webc.get_volume("metadata").unwrap();

        assert_eq!(
            meta_volume.read_file("LICENSE").unwrap(),
            (b"license".as_slice().into(), Some(license_hash)),
        );
        assert_eq!(
            meta_volume.read_file("README.md").unwrap(),
            (b"readme".as_slice().into(), Some(readme_hash)),
        );
        assert_eq!(
            dir1_volume.read_file("a").unwrap(),
            (b"a".as_slice().into(), Some(a_hash))
        );
        assert_eq!(
            dir1_volume.read_file("b").unwrap(),
            (b"b".as_slice().into(), Some(b_hash))
        );
    }

    #[test]
    fn serialize_package_with_nested_fs_entries_without_local_base_fs_paths() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"
                readme = 'README.md'
                license-file = 'LICENSE'

                [fs]
                "/path_in_wasix" = "local-dir/dir1"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();

        // Now we want to set up the following filesystem tree:
        //
        // - local-dir/
        //   - dir1/
        //     - dir2/
        //       - a
        //     - b
        let local_dir = temp.path().join("local-dir");
        std::fs::create_dir_all(&local_dir).unwrap();

        let dir1 = local_dir.join("dir1");
        std::fs::create_dir_all(&dir1).unwrap();

        let dir2 = dir1.join("dir2");
        std::fs::create_dir_all(&dir2).unwrap();

        let a = dir2.join("a");
        let b = dir1.join("b");

        std::fs::write(a, "a").unwrap();
        std::fs::write(b, "b").unwrap();

        let package = Package::from_manifest(manifest).unwrap();

        let webc = package.serialize().unwrap();
        let webc = from_bytes(webc).unwrap();
        let manifest = webc.manifest();
        let wapm_metadata = manifest.wapm().unwrap().unwrap();

        assert!(wapm_metadata.name.is_none());
        assert!(wapm_metadata.version.is_none());
        assert!(wapm_metadata.description.is_none());

        let fs_table = manifest.filesystem().unwrap().unwrap();
        assert_eq!(
            fs_table,
            [FileSystemMapping {
                from: None,
                volume_name: "/local-dir/dir1".to_string(),
                host_path: None,
                mount_path: "/path_in_wasix".to_string(),
            },]
        );

        let readme_hash: [u8; 32] = sha2::Sha256::digest(b"readme").into();
        let license_hash: [u8; 32] = sha2::Sha256::digest(b"license").into();

        let a_hash: [u8; 32] = sha2::Sha256::digest(b"a").into();
        let dir2_hash: [u8; 32] = sha2::Sha256::digest(a_hash).into();
        let b_hash: [u8; 32] = sha2::Sha256::digest(b"b").into();

        let dir1_volume = webc.get_volume("/local-dir/dir1").unwrap();
        let meta_volume = webc.get_volume("metadata").unwrap();

        assert_eq!(
            meta_volume.read_file("LICENSE").unwrap(),
            (b"license".as_slice().into(), Some(license_hash)),
        );
        assert_eq!(
            meta_volume.read_file("README.md").unwrap(),
            (b"readme".as_slice().into(), Some(readme_hash)),
        );
        assert_eq!(
            dir1_volume
                .read_dir("/")
                .unwrap()
                .into_iter()
                .map(|(p, h, _)| (p, h))
                .collect::<Vec<_>>(),
            vec![
                (PathSegment::parse("b").unwrap(), Some(b_hash)),
                (PathSegment::parse("dir2").unwrap(), Some(dir2_hash))
            ]
        );
        assert_eq!(
            dir1_volume
                .read_dir("/dir2")
                .unwrap()
                .into_iter()
                .map(|(p, h, _)| (p, h))
                .collect::<Vec<_>>(),
            vec![(PathSegment::parse("a").unwrap(), Some(a_hash))]
        );
        assert_eq!(
            dir1_volume.read_file("/dir2/a").unwrap(),
            (b"a".as_slice().into(), Some(a_hash))
        );
        assert_eq!(
            dir1_volume.read_file("/b").unwrap(),
            (b"b".as_slice().into(), Some(b_hash))
        );
    }

    #[test]
    fn serialize_package_mapped_to_same_dir_without_local_base_fs_paths() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"
                readme = 'README.md'
                license-file = 'LICENSE'

                [fs]
                "/dir1" = "local-dir1/dir"
                "/dir2" = "local-dir2/dir"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();

        // Now we want to set up the following filesystem tree:
        //
        // - local-dir1/
        //   - dir
        // - local-dir2/
        //   - dir
        let dir1 = temp.path().join("local-dir1").join("dir");
        std::fs::create_dir_all(&dir1).unwrap();
        let dir2 = temp.path().join("local-dir2").join("dir");
        std::fs::create_dir_all(&dir2).unwrap();

        let package = Package::from_manifest(manifest).unwrap();

        let webc = package.serialize().unwrap();
        let webc = from_bytes(webc).unwrap();
        let manifest = webc.manifest();
        let wapm_metadata = manifest.wapm().unwrap().unwrap();

        assert!(wapm_metadata.name.is_none());
        assert!(wapm_metadata.version.is_none());
        assert!(wapm_metadata.description.is_none());

        let fs_table = manifest.filesystem().unwrap().unwrap();
        assert_eq!(
            fs_table,
            [
                FileSystemMapping {
                    from: None,
                    volume_name: "/local-dir1/dir".to_string(),
                    host_path: None,
                    mount_path: "/dir1".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "/local-dir2/dir".to_string(),
                    host_path: None,
                    mount_path: "/dir2".to_string(),
                },
            ]
        );

        let readme_hash: [u8; 32] = sha2::Sha256::digest(b"readme").into();
        let license_hash: [u8; 32] = sha2::Sha256::digest(b"license").into();

        let dir1_volume = webc.get_volume("/local-dir1/dir").unwrap();
        let dir2_volume = webc.get_volume("/local-dir2/dir").unwrap();
        let meta_volume = webc.get_volume("metadata").unwrap();

        assert_eq!(
            meta_volume.read_file("LICENSE").unwrap(),
            (b"license".as_slice().into(), Some(license_hash)),
        );
        assert_eq!(
            meta_volume.read_file("README.md").unwrap(),
            (b"readme".as_slice().into(), Some(readme_hash)),
        );
        assert!(dir1_volume.read_dir("/").unwrap().is_empty());
        assert!(dir2_volume.read_dir("/").unwrap().is_empty());
    }

    #[test]
    fn metadata_only_contains_relevant_files() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            license-file = "./path/to/LICENSE"
            readme = "README.md"

            [[module]]
            name = "asdf"
            source = "asdf.wasm"
            abi = "none"
            bindings = { wai-version = "0.2.0", exports = "asdf.wai", imports = ["browser.wai"] }
        "#;

        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        let license_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&license_dir).unwrap();
        std::fs::write(license_dir.join("LICENSE"), "license").unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("asdf.wasm"), b"\0asm...").unwrap();
        std::fs::write(temp.path().join("asdf.wai"), "exports").unwrap();
        std::fs::write(temp.path().join("browser.wai"), "imports").unwrap();
        std::fs::write(temp.path().join("unwanted_file.txt"), "unwanted_file").unwrap();

        let package = Package::from_manifest(manifest).unwrap();

        let contents: Vec<_> = package
            .get_volume("metadata")
            .unwrap()
            .read_dir(&PathSegments::ROOT)
            .unwrap()
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();

        assert_eq!(
            contents,
            vec![
                PathSegment::parse("README.md").unwrap(),
                PathSegment::parse("asdf.wai").unwrap(),
                PathSegment::parse("browser.wai").unwrap(),
                PathSegment::parse("path").unwrap(),
            ]
        );
    }

    #[test]
    fn create_from_in_memory() -> anyhow::Result<()> {
        let wasmer_toml = r#"
            [dependencies]
            "wasmer/python" = "3.12.9+build.9"
            
            
            [[command]]
            module = "wasmer/python:python"
            name = "hello"
            runner = "wasi"
            
            [command.annotations.wasi]
            main-args = [ "-c", "import os; print([f for f in os.walk('/public')]); " ]
            
            [fs]
            "/public" = "public" 
        "#;

        let manifest = toml::from_str(wasmer_toml)?;

        let file_modified = SystemTime::now();
        let file_data = String::from("Hello, world!").as_bytes().to_vec();

        let file = MemoryFile {
            modified: file_modified,
            data: file_data,
        };

        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("hello.txt"), MemoryNode::File(file));

        let dir_modified = SystemTime::now();
        let dir = MemoryDir {
            modified: dir_modified,
            nodes,
        };

        let volume = MemoryVolume { node: dir };
        let mut volumes = BTreeMap::new();

        volumes.insert("public".to_string(), volume);

        let atoms = BTreeMap::new();
        let package = super::Package::from_in_memory(
            manifest,
            volumes,
            atoms,
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::now(),
                    nodes: BTreeMap::new(),
                },
            },
            Strictness::Strict,
        )?;

        _ = package.serialize()?;

        Ok(())
    }

    #[test]
    fn compare_fs_mem_manifest() -> anyhow::Result<()> {
        let wasmer_toml = r#"
            [package]
            name = "test"
            version = "0.0.0"
            description = "asdf"
        "#;

        let temp = TempDir::new()?;
        let manifest_path = temp.path().join("wasmer.toml");
        std::fs::write(&manifest_path, wasmer_toml).unwrap();

        let fs_package = super::Package::from_manifest(manifest_path)?;

        let manifest = toml::from_str(wasmer_toml)?;
        let memory_package = super::Package::from_in_memory(
            manifest,
            Default::default(),
            Default::default(),
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: BTreeMap::new(),
                },
            },
            Strictness::Lossy,
        )?;

        assert_eq!(memory_package.serialize()?, fs_package.serialize()?);

        Ok(())
    }

    #[test]
    fn compare_fs_mem_manifest_and_atoms() -> anyhow::Result<()> {
        let wasmer_toml = r#"
            [package]
            name = "test"
            version = "0.0.0"
            description = "asdf"

            [[module]]
            name = "foo"
            source = "foo.wasm"
            abi = "wasi"
        "#;

        let temp = TempDir::new()?;
        let manifest_path = temp.path().join("wasmer.toml");
        std::fs::write(&manifest_path, wasmer_toml).unwrap();

        let atom_path = temp.path().join("foo.wasm");
        std::fs::write(&atom_path, b"").unwrap();

        let fs_package = super::Package::from_manifest(manifest_path)?;

        let manifest = toml::from_str(wasmer_toml)?;
        let mut atoms = BTreeMap::new();
        atoms.insert("foo".to_owned(), (None, OwnedBuffer::new()));
        let memory_package = super::Package::from_in_memory(
            manifest,
            Default::default(),
            atoms,
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: BTreeMap::new(),
                },
            },
            Strictness::Lossy,
        )?;

        assert_eq!(memory_package.serialize()?, fs_package.serialize()?);

        Ok(())
    }

    #[test]
    fn compare_fs_mem_volume() -> anyhow::Result<()> {
        let wasmer_toml = r#"
            [package]
            name = "test"
            version = "0.0.0"
            description = "asdf"

            [[module]]
            name = "foo"
            source = "foo.wasm"
            abi = "wasi"

            [fs]
            "/bar" = "bar"
        "#;

        let temp = TempDir::new()?;
        let manifest_path = temp.path().join("wasmer.toml");
        std::fs::write(&manifest_path, wasmer_toml).unwrap();

        let atom_path = temp.path().join("foo.wasm");
        std::fs::write(&atom_path, b"").unwrap();

        let bar = temp.path().join("bar");
        std::fs::create_dir(&bar).unwrap();

        let baz = bar.join("baz");
        std::fs::write(&baz, b"abc")?;

        let baz_metadata = std::fs::metadata(&baz)?;

        let fs_package = super::Package::from_manifest(manifest_path)?;

        let manifest = toml::from_str(wasmer_toml)?;

        let mut atoms = BTreeMap::new();
        atoms.insert("foo".to_owned(), (None, OwnedBuffer::new()));

        let mut volumes = BTreeMap::new();
        volumes.insert(
            "/bar".to_owned(),
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: {
                        let mut children = BTreeMap::new();

                        children.insert(
                            "baz".to_owned(),
                            MemoryNode::File(MemoryFile {
                                modified: baz_metadata.modified()?,
                                data: b"abc".to_vec(),
                            }),
                        );

                        children
                    },
                },
            },
        );
        let memory_package = super::Package::from_in_memory(
            manifest,
            volumes,
            atoms,
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: Default::default(),
                },
            },
            Strictness::Lossy,
        )?;

        assert_eq!(memory_package.serialize()?, fs_package.serialize()?);

        Ok(())
    }

    #[test]
    fn compare_fs_mem_bindings() -> anyhow::Result<()> {
        let temp = TempDir::new().unwrap();

        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            license-file = "LICENSE"
            readme = "README.md"

            [[module]]
            name = "asdf"
            source = "asdf.wasm"
            abi = "none"
            bindings = { wai-version = "0.2.0", exports = "asdf.wai", imports = ["browser.wai"] }

            [fs]
            "/dir1" = "local-dir1/dir"
            "/dir2" = "local-dir2/dir"
        "#;

        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        std::fs::write(temp.path().join("LICENSE"), "license").unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("asdf.wasm"), b"\0asm...").unwrap();
        std::fs::write(temp.path().join("asdf.wai"), "exports").unwrap();
        std::fs::write(temp.path().join("browser.wai"), "imports").unwrap();

        // Now we want to set up the following filesystem tree:
        //
        // - local-dir1/
        //   - dir
        // - local-dir2/
        //   - dir
        let dir1 = temp.path().join("local-dir1").join("dir");
        std::fs::create_dir_all(&dir1).unwrap();
        let dir2 = temp.path().join("local-dir2").join("dir");
        std::fs::create_dir_all(&dir2).unwrap();

        let fs_package = super::Package::from_manifest(manifest)?;

        let manifest = toml::from_str(wasmer_toml)?;

        let mut atoms = BTreeMap::new();
        atoms.insert(
            "asdf".to_owned(),
            (None, OwnedBuffer::from_static(b"\0asm...")),
        );

        let mut volumes = BTreeMap::new();
        volumes.insert(
            "/local-dir1/dir".to_owned(),
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: Default::default(),
                },
            },
        );
        volumes.insert(
            "/local-dir2/dir".to_owned(),
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: Default::default(),
                },
            },
        );

        let memory_package = super::Package::from_in_memory(
            manifest,
            volumes,
            atoms,
            MemoryVolume {
                node: MemoryDir {
                    modified: SystemTime::UNIX_EPOCH,
                    nodes: {
                        let mut children = BTreeMap::new();

                        children.insert(
                            "README.md".to_owned(),
                            MemoryNode::File(MemoryFile {
                                modified: temp.path().join("README.md").metadata()?.modified()?,
                                data: b"readme".to_vec(),
                            }),
                        );

                        children.insert(
                            "LICENSE".to_owned(),
                            MemoryNode::File(MemoryFile {
                                modified: temp.path().join("LICENSE").metadata()?.modified()?,
                                data: b"license".to_vec(),
                            }),
                        );

                        children.insert(
                            "asdf.wai".to_owned(),
                            MemoryNode::File(MemoryFile {
                                modified: temp.path().join("asdf.wai").metadata()?.modified()?,
                                data: b"exports".to_vec(),
                            }),
                        );

                        children.insert(
                            "browser.wai".to_owned(),
                            MemoryNode::File(MemoryFile {
                                modified: temp.path().join("browser.wai").metadata()?.modified()?,
                                data: b"imports".to_vec(),
                            }),
                        );

                        children
                    },
                },
            },
            Strictness::Lossy,
        )?;

        let memory_package = memory_package.serialize()?;
        let fs_package = fs_package.serialize()?;

        assert_eq!(memory_package, fs_package);

        Ok(())
    }
}
