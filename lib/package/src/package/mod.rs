//! Load a Wasmer package from disk.
pub(crate) mod manifest;
#[allow(clippy::module_inception)]
pub(crate) mod package;
pub(crate) mod strictness;
pub(crate) mod volume;

pub use self::{
    manifest::ManifestError,
    package::{Package, WasmerPackageError},
    strictness::Strictness,
    volume::{fs::*, in_memory::*, WasmerPackageVolume},
};

#[cfg(test)]
mod tests {
    use sha2::Digest;
    use shared_buffer::OwnedBuffer;
    use tempfile::TempDir;

    use webc::{
        metadata::annotations::FileSystemMapping,
        migration::{are_semantically_equivalent, v2_to_v3, v3_to_v2},
    };

    use crate::{package::Package, utils::from_bytes};

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

        let container = from_bytes(webc_v2.clone().into_bytes()).unwrap();
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

        let container = from_bytes(webc_v3.into_bytes()).unwrap();
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
        let container = from_bytes(webc_v2.clone().into_bytes()).unwrap();
        let manifest = container.manifest();
        assert!(manifest.filesystem().unwrap().is_none());

        // Go back to v3
        let webc_v3 = v2_to_v3(webc_v2).unwrap();
        let container = from_bytes(webc_v3.into_bytes()).unwrap();
        let manifest = container.manifest();
        assert!(manifest.filesystem().unwrap().is_none());
    }

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
        let container = from_bytes(webc).unwrap();

        let out_dir = temp.path().join("out");
        container.unpack(&out_dir, false).unwrap();

        let expected_entries = [
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
