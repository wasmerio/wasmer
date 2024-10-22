//! Contains code for migrating v2 <--> v3

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;

use shared_buffer::OwnedBuffer;

use webc::metadata::annotations::FileSystemMapping;
use webc::v2;
use webc::v3;
use webc::PathSegment;
use webc::PathSegments;
use webc::ToPathSegments;

use crate::package::manifest::sanitize_path;
use crate::package::volume::abstract_volume::Metadata;
use crate::package::volume::AbstractVolume;

fn v2_to_v3_directory(dir: v2::write::Directory<'_>) -> v3::write::Directory<'_> {
    let mut children = BTreeMap::new();

    for (path, entry) in dir.children {
        let entry = match entry {
            v2::write::DirEntry::Dir(dir) => {
                v3::write::volumes::DirEntry::Dir(v2_to_v3_directory(dir))
            }
            v2::write::DirEntry::File(file) => {
                let file = match file {
                    v2::write::FileEntry::Borrowed(b) => {
                        v3::write::FileEntry::borrowed(b, v3::Timestamps::default())
                    }
                    v2::write::FileEntry::Owned(o) => {
                        v3::write::FileEntry::owned(o, v3::Timestamps::default())
                    }
                    v2::write::FileEntry::Reader(r) => {
                        v3::write::FileEntry::reader(r, v3::Timestamps::default())
                    }
                };

                v3::write::volumes::DirEntry::File(file)
            }
        };

        children.insert(path, entry);
    }

    v3::write::Directory::new(children, v3::Timestamps::default())
}

fn v3_to_v2_directory(dir: v3::write::Directory<'_>) -> v2::write::Directory<'_> {
    let mut children = BTreeMap::new();

    for (path, entry) in dir.children {
        let entry = match entry {
            v3::write::DirEntry::Dir(dir) => v2::write::DirEntry::Dir(v3_to_v2_directory(dir)),
            v3::write::DirEntry::File(file) => {
                let file = match file.content {
                    v3::write::volumes::FileContent::Borrowed(b) => {
                        v2::write::FileEntry::Borrowed(b)
                    }
                    v3::write::volumes::FileContent::Owned(o) => v2::write::FileEntry::Owned(o),
                    v3::write::volumes::FileContent::Reader(r) => v2::write::FileEntry::Reader(r),
                };

                v2::write::DirEntry::File(file)
            }
        };

        children.insert(path, entry);
    }

    v2::write::Directory { children }
}

/// Migrates WebC V2 to V3
pub fn v2_to_v3(webc: impl Into<OwnedBuffer>) -> Result<OwnedBuffer, anyhow::Error> {
    let reader = v2::read::OwnedReader::parse(webc)?;

    let mut manifest = reader.manifest().clone();

    let mut fs_mappings = manifest.filesystem()?;

    if let Some(fs_mappings) = fs_mappings.as_mut() {
        for mapping in fs_mappings.0.iter_mut() {
            // unwrap safety:
            //
            // `host_path` must be present in v2.
            mapping.volume_name = mapping.host_path.take().unwrap();
        }
        manifest.update_filesystem(fs_mappings.clone())?;
    }

    let atoms = reader
        .iter_atoms()
        .map(|(name, data)| {
            let path = PathSegment::parse(name).unwrap();
            let file_entry =
                v3::write::FileEntry::owned(data.clone().into_bytes(), v3::Timestamps::default());

            (path, file_entry)
        })
        .collect::<BTreeMap<PathSegment, v3::write::FileEntry<'_>>>();

    let writer = v3::write::Writer::new(v3::ChecksumAlgorithm::Sha256);
    let mut writer = writer.write_manifest(&manifest)?.write_atoms(atoms)?;

    let mut volumes = BTreeMap::new();

    for entry in reader.iter_volumes() {
        let (name, section) = entry?;

        let mut root: v2::write::Directory<'static> = section.root()?.try_into()?;

        if name == "atom" {
            if let Some(fs_mappings) = fs_mappings.as_ref() {
                for FileSystemMapping { volume_name, .. } in fs_mappings.iter() {
                    let path_segments = volume_name.to_path_segments()?;

                    let mut curr_dir = &mut root;
                    for segment in path_segments {
                        let v2::write::DirEntry::Dir(dir) = curr_dir
                            .children
                            .get_mut(&segment)
                            .expect("{segment:?} is expected")
                        else {
                            panic!("{segment:?} must be a directory");
                        };

                        curr_dir = dir;
                    }

                    let curr_dir = std::mem::take(curr_dir);

                    let volume = v2_to_v3_directory(curr_dir);

                    volumes.insert(volume_name.clone(), volume);
                }
            }
        } else if name == "metadata" {
            let root = v2_to_v3_directory(root);

            volumes.insert("metadata".to_string(), root);
        } else {
            panic!("Unknown volume {name:?}: webc v2 should only have a metadata volume and an atom volume")
        }
    }

    for (name, volume) in volumes {
        writer.write_volume(name.as_str(), volume)?;
    }

    writer
        .finish(v3::SignatureAlgorithm::None)
        .map(OwnedBuffer::from)
        .map_err(anyhow::Error::from)
}

/// Migrates WebC V3 to V2
pub fn v3_to_v2(webc: impl Into<OwnedBuffer>) -> Result<OwnedBuffer, anyhow::Error> {
    let reader = v3::read::OwnedReader::parse(webc)?;

    let mut manifest = reader.manifest().clone();

    if let Some(mut fs_mappings) = manifest.filesystem()? {
        for mapping in fs_mappings.0.iter_mut() {
            mapping.host_path = Some(mapping.volume_name.clone());

            mapping.volume_name = "atom".to_string();
        }
        manifest.update_filesystem(fs_mappings)?;
    }

    let atoms = reader
        .iter_atoms()
        .map(|(name, _hash, data)| {
            let path = PathSegment::parse(name).unwrap();
            let file = v2::write::FileEntry::Owned(data.clone().into_bytes());

            (path, file)
        })
        .collect::<BTreeMap<PathSegment, v2::write::FileEntry<'_>>>();

    let writer = v2::write::Writer::new(v2::ChecksumAlgorithm::Sha256);
    let mut writer = writer.write_manifest(&manifest)?.write_atoms(atoms)?;

    let mut root = v2::write::Directory::default();
    for entry in reader.iter_volumes() {
        let (name, section) = entry?;

        let volume: v3::write::Directory<'static> = section.root()?.try_into()?;
        let volume = v3_to_v2_directory(volume);

        if name == "metadata" {
            writer.write_volume("metadata", volume)?;
        } else {
            let path_segments = name.to_path_segments()?;

            let segments: Vec<_> = path_segments.iter().collect();

            let mut directory = volume;
            for (index, segment) in segments.iter().enumerate().rev() {
                if index != 0 {
                    let mut temp = v2::write::Directory::default();
                    temp.children
                        .insert((*segment).clone(), v2::write::DirEntry::Dir(directory));
                    directory = temp;
                } else {
                    root.children
                        .insert((*segment).clone(), v2::write::DirEntry::Dir(directory));

                    break;
                }
            }
        }
    }

    writer.write_volume("atom", root)?;

    writer
        .finish(v2::SignatureAlgorithm::None)
        .map(OwnedBuffer::from)
        .map_err(anyhow::Error::from)
}

/// Checks whether two webcs (one v2 and one v3) are semantically equivalent. For v2 and v3 to be equivalent,
/// these criteria must be true:
/// * Contents of the manifest must be the same. In particular, these items:
///     * `origin`
///     * `atoms`
///     * `commands`
///     * `entrypoint`
/// * Atoms in both webcs must be the same:
///     * same number of atoms
///     * same atom names
///     * same atom contents
/// * The filesystem in both webcs must be the same (regardless of structure of volumes):
/// Each top level directory in webc v2 is equivalent to a volume in webc v3 so this check, for
/// every top level directory in v2, will find the associated volume in v3 and walk them
/// at the same time to make they have the same entries and contents.
pub fn are_semantically_equivalent(v2: OwnedBuffer, v3: OwnedBuffer) -> Result<(), anyhow::Error> {
    let v2_reader = webc::v2::read::OwnedReader::parse(v2)?;
    let v3_reader = webc::v3::read::OwnedReader::parse(v3)?;

    // check manifest
    let v2_manifest = v2_reader.manifest();
    let v3_manifest = v3_reader.manifest();
    if v2_manifest.origin != v3_manifest.origin {
        anyhow::bail!("webcs have different origins");
    }
    if v2_manifest.atoms != v3_manifest.atoms {
        anyhow::bail!("webcs have different atoms");
    }
    if v2_manifest.commands != v3_manifest.commands {
        anyhow::bail!("webcs have different commands");
    }
    if v2_manifest.entrypoint != v3_manifest.entrypoint {
        anyhow::bail!("webcs have different entrypoints");
    }

    // check atoms
    let v2_atoms = v2_reader.atom_names().collect::<HashSet<_>>();
    let v3_atoms = v3_reader.atom_names().collect::<HashSet<_>>();
    if v2_atoms.len() != v3_atoms.len() {
        anyhow::bail!(
            "webcs do not have the same number of atoms: {} != {}",
            v2_atoms.len(),
            v3_atoms.len()
        );
    }
    if v2_atoms != v3_atoms {
        anyhow::bail!(
            "webcs do not have the same atom names:\n{:?}\n!=\n{:?}",
            v2_atoms,
            v3_atoms
        );
    }
    for atom_name in v2_atoms {
        let v2_atom = v2_reader
            .get_atom(atom_name)
            .ok_or_else(|| anyhow::anyhow!("failed to get atom: {} from webc v2", atom_name))?;
        let v3_atom = &v3_reader
            .get_atom(atom_name)
            .ok_or_else(|| anyhow::anyhow!("failed to get atom: {} from webc v3", atom_name))?
            .1;

        if v2_atom != v3_atom {
            anyhow::bail!("webcs have different contents for atom: {}", atom_name);
        }
    }

    // chec metadata volume
    let v2_metadata_volume = v2_reader.get_volume("metadata")?;
    let v3_metadata_volume = v3_reader.get_volume("metadata")?;

    let v2_entries = v2_metadata_volume
        .read_dir(&webc::PathSegments::ROOT)
        .ok_or_else(|| {
            anyhow::anyhow!("failed to read the root dir from webc v2 metadata volume")
        })?;
    let v3_entries = v3_metadata_volume
        .read_dir(&webc::PathSegments::ROOT)
        .ok_or_else(|| {
            anyhow::anyhow!("failed to read the root dir from webc v3 metadata volume",)
        })?;

    walk_dir(
        &v2_metadata_volume,
        PathSegments::ROOT,
        v2_entries,
        &v3_metadata_volume,
        PathSegments::ROOT,
        v3_entries,
    )?;

    // check the filesystem
    if let Some(fs_map) = v2_reader.manifest().filesystem()? {
        let v2_volume = v2_reader.get_volume("atom")?;

        for FileSystemMapping { host_path, .. } in fs_map {
            let host_path = host_path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("failed to get host_path from webc v2"))?;
            let v2_target_dir = webc::PathSegments::from_str(host_path)?;
            let v3_volume_name = sanitize_path(host_path);

            let v3_volume = v3_reader.get_volume(&v3_volume_name)?;

            let v2_entries = v2_volume.read_dir(&v2_target_dir).ok_or_else(|| {
                anyhow::anyhow!("failed to read dir: {} from webc v2", v2_target_dir)
            })?;
            let v3_entries = v3_volume
                .read_dir(&webc::PathSegments::ROOT)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to read the root dir from webc v3 volume: {}",
                        v3_volume_name
                    )
                })?;

            walk_dir(
                &v2_volume,
                v2_target_dir,
                v2_entries,
                &v3_volume,
                webc::PathSegments::ROOT,
                v3_entries,
            )?;
        }
    } else {
        let volumes = v3_reader
            .volume_names()
            .filter(|name| *name != "metadata")
            .collect::<Vec<_>>();

        if !volumes.is_empty() {
            anyhow::bail!(
                "webc v2 has no fs entry, but webc v3 has non-metadata volumes: {volumes:?}"
            );
        }
    }
    Ok(())
}

fn walk_dir(
    v2_volume: &webc::v2::read::VolumeSection,
    v2_base: PathSegments,
    v2_entries: Vec<(PathSegment, Option<[u8; 32]>, Metadata)>,
    v3_volume: &webc::v3::read::VolumeSection,
    v3_base: PathSegments,
    v3_entries: Vec<(PathSegment, Option<[u8; 32]>, Metadata)>,
) -> Result<(), anyhow::Error> {
    let v2_entries: HashMap<_, _> =
        HashMap::from_iter(v2_entries.into_iter().map(|(n, _, m)| (n, m)));
    let v3_entries: HashMap<_, _> =
        HashMap::from_iter(v3_entries.into_iter().map(|(n, _, m)| (n, m)));
    if v2_entries.len() != v3_entries.len() {
        anyhow::bail!(
            "webcs have different number of entries: {} != {}",
            v2_entries.len(),
            v3_entries.len()
        );
    }

    if v2_entries.is_empty() {
        return Ok(());
    }

    for (v2_entry, v2_metadata) in v2_entries {
        let v3_metadata = v3_entries.get(&v2_entry).ok_or_else(|| {
            anyhow::anyhow!(
                "webc v2 has entry: {}, but webc v3 does not have it in volume: {} at {}",
                v2_base.join(v2_entry.clone()),
                v3_volume.name(),
                v3_base.join(v2_entry.clone())
            )
        })?;

        match (v2_metadata, v3_metadata) {
            (Metadata::Dir { .. }, Metadata::Dir { .. }) => {
                let v2_base = v2_base.join(v2_entry.clone());
                let v3_base = v3_base.join(v2_entry.clone());

                let v2_entries = v2_volume
                    .read_dir(&v2_base)
                    .ok_or_else(|| anyhow::anyhow!("failed to read dir: {} in webc v2", v2_base))?;
                let v3_entries = v3_volume.read_dir(&v3_base).ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to read dir: {} in webc v3 in volume: {}",
                        v3_base,
                        v3_volume.name()
                    )
                })?;

                walk_dir(
                    v2_volume, v2_base, v2_entries, v3_volume, v3_base, v3_entries,
                )?;
            }
            (Metadata::Dir { .. }, Metadata::File { .. }) => {
                anyhow::bail!("webc v2 has a dir, but webc v3 has a file")
            }
            (Metadata::File { .. }, Metadata::Dir { .. }) => {
                anyhow::bail!("webc v2 has a file, but webc v3 has a dir")
            }
            (
                Metadata::File {
                    length: v2_length, ..
                },
                Metadata::File {
                    length: v3_length, ..
                },
            ) => {
                if v2_length != *v3_length {
                    anyhow::bail!(
                        "webcs have different length for {} in v2 ({} in volume {} in v3): {} != {}",
                        v2_base.join(v2_entry.clone()),
                        v3_base.join(v2_entry.clone()),
                        v3_volume.name(),
                        v2_length,
                        v3_length
                    );
                }

                let v2_file_path = v2_base.join(v2_entry.clone());
                let v3_file_path = v3_base.join(v2_entry.clone());

                let v2_file = v2_volume.lookup_file(v2_file_path)?;
                let v3_file = v3_volume.lookup_file(v3_file_path)?.0;

                if v2_file != v3_file {
                    anyhow::bail!("webcs have different content for file");
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sha2::Digest;
    use shared_buffer::OwnedBuffer;
    use tempfile::TempDir;

    use crate::{container::Container, package::Package};

    use super::{are_semantically_equivalent, v2_to_v3, v3_to_v2};
    use webc::metadata::annotations::FileSystemMapping;

    #[test]
    fn migration_roundtrip() {
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

        let webc_v2 = v3_to_v2(webc.clone()).unwrap();

        are_semantically_equivalent(webc_v2.clone(), webc.into()).unwrap();

        let container = Container::from_bytes(webc_v2.clone().into_bytes()).unwrap();
        let manifest = container.manifest();
        let fs_table = manifest.filesystem().unwrap().unwrap();
        assert_eq!(
            fs_table,
            [
                FileSystemMapping {
                    from: None,
                    volume_name: "atom".to_string(),
                    host_path: Some("/first".to_string()),
                    mount_path: "/first".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "atom".to_string(),
                    host_path: Some("/nested/dir".to_string()),
                    mount_path: "/second".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "atom".to_string(),
                    host_path: Some("/third".to_string()),
                    mount_path: "/second/child".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "atom".to_string(),
                    host_path: Some("/empty".to_string()),
                    mount_path: "/empty".to_string(),
                },
            ]
        );

        let atom_volume = container.get_volume("atom").unwrap();
        assert_eq!(
            atom_volume.read_file("/first/file.txt").unwrap(),
            (OwnedBuffer::from(b"File".as_slice()), None)
        );
        assert_eq!(
            atom_volume.read_file("/nested/dir/README.md").unwrap(),
            (OwnedBuffer::from(b"please".as_slice()), None),
        );
        assert_eq!(
            atom_volume
                .read_file("/nested/dir/another-dir/empty.txt")
                .unwrap(),
            (OwnedBuffer::from(b"".as_slice()), None)
        );
        assert_eq!(
            atom_volume.read_file("/third/file.txt").unwrap(),
            (OwnedBuffer::from(b"Hello, World!".as_slice()), None)
        );
        assert_eq!(
            atom_volume.read_dir("/empty").unwrap().len(),
            0,
            "Directories should be included, even if empty"
        );

        // Go back to v3
        let webc_v3 = v2_to_v3(webc_v2.clone()).unwrap();

        are_semantically_equivalent(webc_v2, webc_v3.clone()).unwrap();

        let container = Container::from_bytes(webc_v3.into_bytes()).unwrap();
        let manifest = container.manifest();
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

        let first_volume = container.get_volume("/first").unwrap();
        assert_eq!(
            first_volume.read_file("/file.txt").unwrap(),
            (b"File".as_slice().into(), Some(first_file_hash)),
        );

        let nested_dir_volume = container.get_volume("/nested/dir").unwrap();
        assert_eq!(
            nested_dir_volume.read_file("README.md").unwrap(),
            (b"please".as_slice().into(), Some(readme_hash)),
        );
        assert_eq!(
            nested_dir_volume
                .read_file("/another-dir/empty.txt")
                .unwrap(),
            (b"".as_slice().into(), Some(empty_hash))
        );

        let third_volume = container.get_volume("/third").unwrap();
        assert_eq!(
            third_volume.read_file("/file.txt").unwrap(),
            (b"Hello, World!".as_slice().into(), Some(third_file_hash))
        );

        let empty_volume = container.get_volume("/empty").unwrap();
        assert_eq!(
            empty_volume.read_dir("/").unwrap().len(),
            0,
            "Directories should be included, even if empty"
        );
    }

    #[test]
    fn fs_entry_is_not_required_for_migration() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
                [package]
                name = "some/package"
                version = "0.0.0"
                description = "Test package"
            "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        let package = Package::from_manifest(manifest).unwrap();

        let webc = package.serialize().unwrap();

        let webc_v2 = v3_to_v2(webc).unwrap();
        let container = Container::from_bytes(webc_v2.clone().into_bytes()).unwrap();
        let manifest = container.manifest();
        assert!(manifest.filesystem().unwrap().is_none());

        // Go back to v3
        let webc_v3 = v2_to_v3(webc_v2).unwrap();
        let container = Container::from_bytes(webc_v3.into_bytes()).unwrap();
        let manifest = container.manifest();
        assert!(manifest.filesystem().unwrap().is_none());
    }
}
