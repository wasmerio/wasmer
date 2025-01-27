use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use shared_buffer::OwnedBuffer;

use webc::{
    sanitize_path,
    v3::{
        self,
        write::{DirEntry, Directory, FileEntry},
    },
    AbstractVolume, Metadata, PathSegment, PathSegments, Timestamps, ToPathSegments,
};

use crate::package::Strictness;

use super::WasmerPackageVolume;

/// A lazily loaded volume in a Wasmer package.
///
/// Note that it is the package resolver's role to interpret a package's
/// [`crate::metadata::annotations::FileSystemMappings`]. A [`Volume`] contains
/// directories as they were when the package was published.
#[derive(Debug, Clone, PartialEq)]
pub struct FsVolume {
    /// Name of the volume
    name: String,
    /// A pre-computed set of intermediate directories that are needed to allow
    /// access to the whitelisted files and directories.
    intermediate_directories: BTreeSet<PathBuf>,
    /// Specific files that this volume has access to.
    metadata_files: BTreeSet<PathBuf>,
    /// Directories that allow the user to access anything inside them.
    mapped_directories: BTreeSet<PathBuf>,
    /// The base directory all [`PathSegments`] will be resolved relative to.
    base_dir: PathBuf,
}

impl FsVolume {
    /// The name of the volume used to store metadata files.
    pub(crate) const METADATA: &'static str = "metadata";

    /// Create a new metadata volume.
    pub(crate) fn new_metadata(
        manifest: &wasmer_config::package::Manifest,
        base_dir: impl Into<PathBuf>,
    ) -> Result<Self, Error> {
        let base_dir = base_dir.into();
        let mut files = BTreeSet::new();

        // check if manifest.package is None
        if let Some(package) = &manifest.package {
            if let Some(license_file) = &package.license_file {
                files.insert(base_dir.join(license_file));
            }

            if let Some(readme) = &package.readme {
                files.insert(base_dir.join(readme));
            }
        }

        for module in &manifest.modules {
            if let Some(bindings) = &module.bindings {
                let bindings_files = bindings.referenced_files(&base_dir)?;
                files.extend(bindings_files);
            }
        }

        Ok(FsVolume::new_with_intermediate_dirs(
            FsVolume::METADATA.to_string(),
            base_dir,
            files,
            BTreeSet::new(),
        ))
    }

    pub(crate) fn new_assets(
        manifest: &wasmer_config::package::Manifest,
        base_dir: &Path,
    ) -> Result<BTreeMap<String, Self>, Error> {
        // Create asset volumes
        let dirs: BTreeSet<_> = manifest
            .fs
            .values()
            .map(|path| base_dir.join(path))
            .collect();

        for path in &dirs {
            // Perform a basic sanity check to make sure the directories exist.
            let _ = std::fs::metadata(path).with_context(|| {
                format!("Unable to get the metadata for \"{}\"", path.display())
            })?;
        }

        let mut volumes = BTreeMap::new();
        for entry in manifest.fs.values() {
            let name = entry
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Failed to convert path to str"))?;

            let name = sanitize_path(name);

            let mut dirs = BTreeSet::new();
            let dir = base_dir.join(entry);
            dirs.insert(dir);

            volumes.insert(
                name.clone(),
                FsVolume::new(
                    name.to_string(),
                    base_dir.to_path_buf(),
                    BTreeSet::new(),
                    dirs,
                ),
            );
        }

        Ok(volumes)
    }

    pub(crate) fn new_with_intermediate_dirs(
        name: String,
        base_dir: PathBuf,
        whitelisted_files: BTreeSet<PathBuf>,
        whitelisted_directories: BTreeSet<PathBuf>,
    ) -> Self {
        let mut intermediate_directories: BTreeSet<PathBuf> = whitelisted_files
            .iter()
            .filter_map(|p| p.parent())
            .chain(whitelisted_directories.iter().map(|p| p.as_path()))
            .flat_map(|dir| dir.ancestors())
            .filter(|dir| dir.starts_with(&base_dir))
            .map(|dir| dir.to_path_buf())
            .collect();

        // The base directory is always accessible (even if its contents isn't)
        intermediate_directories.insert(base_dir.clone());

        FsVolume {
            name,
            intermediate_directories,
            metadata_files: whitelisted_files,
            mapped_directories: whitelisted_directories,
            base_dir,
        }
    }

    pub(crate) fn new(
        name: String,
        base_dir: PathBuf,
        whitelisted_files: BTreeSet<PathBuf>,
        whitelisted_directories: BTreeSet<PathBuf>,
    ) -> Self {
        FsVolume {
            name,
            intermediate_directories: BTreeSet::new(),
            metadata_files: whitelisted_files,
            mapped_directories: whitelisted_directories,
            base_dir,
        }
    }

    fn is_accessible(&self, path: &Path) -> bool {
        self.intermediate_directories.contains(path)
            || self.metadata_files.contains(path)
            || self
                .mapped_directories
                .iter()
                .any(|dir| path.starts_with(dir))
    }

    fn resolve(&self, path: &PathSegments) -> Option<PathBuf> {
        let resolved = if let Some(dir) = &self.mapped_directories.first() {
            resolve(dir, path)
        } else {
            resolve(&self.base_dir, path)
        };

        let accessible = self.is_accessible(&resolved);
        accessible.then_some(resolved)
    }

    /// Returns the name of the volume
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Read a file from the volume.
    pub fn read_file(&self, path: &PathSegments) -> Option<OwnedBuffer> {
        let path = self.resolve(path)?;
        let mut f = File::open(path).ok()?;

        // First we try to mmap it
        if let Ok(mmapped) = OwnedBuffer::from_file(&f) {
            return Some(mmapped);
        }

        // otherwise, fall back to reading the file's contents into memory
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).ok()?;
        Some(OwnedBuffer::from_bytes(buffer))
    }

    /// Read the contents of a directory.
    #[allow(clippy::type_complexity)]
    pub fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        let resolved = self.resolve(path)?;

        let walker = ignore::WalkBuilder::new(&resolved)
            .require_git(true)
            .add_custom_ignore_filename(".wasmerignore")
            .follow_links(false)
            .max_depth(Some(1))
            .build();

        let mut entries = Vec::new();

        for entry in walker {
            let entry = entry.ok()?;
            // Walk returns the root dir as well, we don't want to process it
            if entry.depth() == 0 {
                continue;
            }

            let entry = entry.path();

            if !self.is_accessible(entry) {
                continue;
            }

            let segment: PathSegment = entry.file_name()?.to_str()?.parse().ok()?;

            let path = path.join(segment.clone());
            let metadata = self.metadata(&path)?;
            entries.push((segment, None, metadata));
        }

        entries.sort_by_key(|k| k.0.clone());

        Some(entries)
    }

    /// Get the metadata for a particular item.
    pub fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        let path = self.resolve(path)?;
        let meta = path.metadata().ok()?;

        let timestamps = Timestamps::from_metadata(&meta).unwrap();

        if meta.is_dir() {
            Some(Metadata::Dir {
                timestamps: Some(timestamps),
            })
        } else if meta.is_file() {
            Some(Metadata::File {
                length: meta.len().try_into().ok()?,
                timestamps: Some(timestamps),
            })
        } else {
            None
        }
    }

    pub(crate) fn as_directory_tree(&self, strictness: Strictness) -> Result<Directory<'_>, Error> {
        if self.name() == "metadata" {
            let mut root = Directory::default();

            for file_path in self.metadata_files.iter() {
                if !file_path.exists() || !file_path.is_file() {
                    if strictness.is_strict() {
                        anyhow::bail!("{} does not exist", file_path.display());
                    }

                    // ignore missing metadata
                    continue;
                }
                let path = file_path.strip_prefix(&self.base_dir)?;
                let path = PathBuf::from("/").join(path);
                let segments = path.to_path_segments()?;
                let segments: Vec<_> = segments.iter().collect();

                let file_entry = DirEntry::File(FileEntry::from_path(file_path)?);

                let mut curr_dir = &mut root;
                for (index, segment) in segments.iter().enumerate() {
                    if segments.len() == 1 {
                        curr_dir.children.insert((*segment).clone(), file_entry);
                        break;
                    } else {
                        if index == segments.len() - 1 {
                            curr_dir.children.insert((*segment).clone(), file_entry);
                            break;
                        }

                        let curr_entry = curr_dir
                            .children
                            .entry((*segment).clone())
                            .or_insert(DirEntry::Dir(Directory::default()));
                        let DirEntry::Dir(dir) = curr_entry else {
                            unreachable!()
                        };

                        curr_dir = dir;
                    }
                }
            }

            Ok(root)
        } else {
            let paths: Vec<_> = self.mapped_directories.iter().cloned().collect();
            directory_tree(paths, &self.base_dir, strictness)
        }
    }
}

impl AbstractVolume for FsVolume {
    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        self.read_file(path).map(|c| (c, None))
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.read_dir(path)
    }

    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        self.metadata(path)
    }
}

impl WasmerPackageVolume for FsVolume {
    fn as_directory_tree(&self, strictness: Strictness) -> Result<Directory<'_>, Error> {
        self.as_directory_tree(strictness)
    }
}

/// Resolve a [`PathSegments`] to its equivalent path on disk.
fn resolve(base_dir: &Path, path: &PathSegments) -> PathBuf {
    let mut resolved = base_dir.to_path_buf();
    for segment in path.iter() {
        resolved.push(segment.as_str());
    }

    resolved
}

/// Given a list of absolute paths, create a directory tree relative to some
/// base directory.
fn directory_tree(
    paths: impl IntoIterator<Item = PathBuf>,
    base_dir: &Path,
    strictness: Strictness,
) -> Result<Directory<'static>, Error> {
    let paths: Vec<_> = paths.into_iter().collect();
    let mut root = Directory::default();

    for path in paths {
        if path.is_file() {
            let dir_entry = v3::write::DirEntry::File(v3::write::FileEntry::from_path(&path)?);
            let path = path.strip_prefix(base_dir)?;
            let path_segment = PathSegment::try_from(path.as_os_str())?;

            if root.children.insert(path_segment, dir_entry).is_some() {
                println!("Warning: {path:?} already exists. Overriding the old entry");
            }
        } else {
            match webc::v3::write::Directory::from_path_with_ignore(&path) {
                Ok(dir) => {
                    for (path, child) in dir.children {
                        root.children.insert(path.clone(), child);
                    }
                }
                Err(e) => {
                    let e = Error::from(e);
                    let error = e.context(format!(
                        "Unable to add \"{}\" to the directory tree",
                        path.display()
                    ));
                    strictness.on_error(&path, error)?;
                }
            }
        }
    }

    Ok(root)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use wasmer_config::package::Manifest;

    use super::*;

    #[test]
    fn metadata_volume() {
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
        let wasmer_toml_path = temp.path().join("wasmer.toml");
        std::fs::write(&wasmer_toml_path, wasmer_toml.as_bytes()).unwrap();
        let license_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&license_dir).unwrap();
        std::fs::write(license_dir.join("LICENSE"), "license").unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("asdf.wai"), "exports").unwrap();
        std::fs::write(temp.path().join("browser.wai"), "imports").unwrap();
        let manifest: Manifest = toml::from_str(wasmer_toml).unwrap();

        let volume = FsVolume::new_metadata(&manifest, temp.path().to_path_buf()).unwrap();

        let entries = volume.read_dir(&PathSegments::ROOT).unwrap();
        let expected = [
            PathSegment::parse("README.md").unwrap(),
            PathSegment::parse("asdf.wai").unwrap(),
            PathSegment::parse("browser.wai").unwrap(),
            PathSegment::parse("path").unwrap(),
        ];

        for i in 0..expected.len() {
            assert_eq!(entries[i].0, expected[i]);
            assert!(entries[i].2.timestamps().is_some());
        }

        let license: PathSegments = "/path/to/LICENSE".parse().unwrap();
        assert_eq!(
            String::from_utf8(volume.read_file(&license).unwrap().into()).unwrap(),
            "license"
        );
    }

    #[test]
    fn asset_volume() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            license_file = "./path/to/LICENSE"
            readme = "README.md"

            [[module]]
            name = "asdf"
            source = "asdf.wasm"
            abi = "none"
            bindings = { wai-version = "0.2.0", exports = "asdf.wai", imports = ["browser.wai"] }

            [fs]
            "/etc" = "etc"
        "#;
        let license_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&license_dir).unwrap();
        std::fs::write(license_dir.join("LICENSE"), "license").unwrap();
        std::fs::write(temp.path().join("README.md"), "readme").unwrap();
        std::fs::write(temp.path().join("asdf.wai"), "exports").unwrap();
        std::fs::write(temp.path().join("browser.wai"), "imports").unwrap();

        let etc = temp.path().join("etc");
        let share = etc.join("share");
        std::fs::create_dir_all(&share).unwrap();

        std::fs::write(etc.join(".wasmerignore"), b"ignore_me").unwrap();
        std::fs::write(etc.join(".hidden"), "anything, really").unwrap();
        std::fs::write(etc.join("ignore_me"), "I should be ignored").unwrap();
        std::fs::write(share.join("package.1"), "man page").unwrap();
        std::fs::write(share.join("ignore_me"), "I should be ignored too").unwrap();

        let manifest: Manifest = toml::from_str(wasmer_toml).unwrap();

        let volume = FsVolume::new_assets(&manifest, temp.path()).unwrap();

        let volume = &volume["/etc"];

        let entries = volume.read_dir(&PathSegments::ROOT).unwrap();
        let expected = [PathSegment::parse("share").unwrap()];

        for i in 0..expected.len() {
            assert_eq!(entries[i].0, expected[i]);
            assert!(entries[i].2.timestamps().is_some());
        }

        let man_page: PathSegments = "/share/package.1".parse().unwrap();
        assert_eq!(
            String::from_utf8(volume.read_file(&man_page).unwrap().into()).unwrap(),
            "man page"
        );
    }
}
