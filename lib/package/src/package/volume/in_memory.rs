use std::{
    collections::BTreeMap,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use webc::{
    v3::{self, write::FileEntry},
    AbstractVolume, Metadata, PathSegment, PathSegments,
};

use crate::package::Strictness;

use super::WasmerPackageVolume;

/// An in-memory representation of a volume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryVolume {
    /// The internal node
    pub node: MemoryDir,
}

impl MemoryVolume {
    /// The name of the volume used to store metadata files.
    pub(crate) const METADATA: &'static str = "metadata";
}

/// An in-memory representation of a filesystem node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MemoryNode {
    /// A file
    File(MemoryFile),

    /// A directory
    Dir(MemoryDir),
}

impl MemoryNode {
    /// Try to return a [`MemoryDir`] out of [`self`].
    pub fn as_dir(&self) -> Option<&MemoryDir> {
        match self {
            MemoryNode::Dir(d) => Some(d),
            _ => None,
        }
    }

    /// Try to return a [`MemoryFile`] out of [`self`].
    pub fn as_file(&self) -> Option<&MemoryFile> {
        match self {
            MemoryNode::File(f) => Some(f),
            _ => None,
        }
    }

    fn as_dir_entry(&self) -> anyhow::Result<webc::v3::write::DirEntry<'_>> {
        match self {
            MemoryNode::File(f) => f.as_dir_entry(),
            MemoryNode::Dir(d) => d.as_dir_entry(),
        }
    }

    fn metadata(&self) -> Metadata {
        match self {
            MemoryNode::File(f) => f.metadata(),
            MemoryNode::Dir(d) => d.metadata(),
        }
    }
}

/// An in-memory file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryFile {
    /// When the file was last modified.
    pub modified: SystemTime,
    /// Raw data  
    pub data: Vec<u8>,
}
impl MemoryFile {
    fn as_dir_entry(&self) -> anyhow::Result<v3::write::DirEntry<'_>> {
        Ok(v3::write::DirEntry::File(FileEntry::owned(
            self.data.clone(),
            v3::Timestamps {
                modified: self.modified,
            },
        )))
    }

    fn metadata(&self) -> Metadata {
        let modified = self.modified.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        Metadata::File {
            length: self.data.len(),
            timestamps: Some(webc::Timestamps::from_modified(modified)),
        }
    }
}

/// An in-memory directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryDir {
    /// When the directory or its contents were last modified.
    pub modified: SystemTime,
    /// List of nodes in the directory
    pub nodes: BTreeMap<String, MemoryNode>,
}

impl MemoryDir {
    fn metadata(&self) -> Metadata {
        let modified = self.modified.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        Metadata::Dir {
            timestamps: Some(webc::Timestamps::from_modified(modified)),
        }
    }

    // Can't return a reference to MemoryNode as it can return itself.
    fn find_node(&self, path: &PathSegments) -> Option<MemoryNode> {
        let mut segments = path.iter().collect::<Vec<_>>();
        if segments.is_empty() {
            return Some(MemoryNode::Dir(self.clone()));
        }

        let mut dir = self;

        while !segments.is_empty() {
            let next = (*segments.first().unwrap()).clone();
            segments.remove(0);

            if let Some(next_node) = dir.nodes.get(&next.to_string()) {
                if segments.is_empty() {
                    return Some(next_node.clone());
                } else {
                    match next_node {
                        MemoryNode::File(_) => break,
                        MemoryNode::Dir(d) => dir = d,
                    }
                }
            }
        }

        None
    }

    fn read_file(&self, path: &PathSegments) -> Option<shared_buffer::OwnedBuffer> {
        self.find_node(path).and_then(|n| {
            if let MemoryNode::File(f) = n {
                Some(shared_buffer::OwnedBuffer::from_bytes(f.data.clone()))
            } else {
                None
            }
        })
    }

    #[allow(clippy::type_complexity)]
    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.find_node(path).and_then(|n| {
            if let MemoryNode::Dir(d) = n {
                let mut ret = vec![];

                for (name, node) in &d.nodes {
                    let meta = node.metadata();
                    ret.push((PathSegment::from_str(name).ok()?, None, meta))
                }

                Some(ret)
            } else {
                None
            }
        })
    }

    fn find_meta(&self, path: &PathSegments) -> Option<Metadata> {
        self.find_node(path).map(|n| n.metadata())
    }

    fn as_directory_tree(
        &self,
        _strictness: Strictness,
    ) -> Result<webc::v3::write::Directory<'_>, anyhow::Error> {
        let mut children = BTreeMap::new();

        for (key, value) in self.nodes.iter() {
            children.insert(PathSegment::from_str(key)?, value.as_dir_entry()?);
        }

        let dir = v3::write::Directory::new(
            children,
            v3::Timestamps {
                modified: self.modified,
            },
        );

        Ok(dir)
    }

    fn as_dir_entry(&self) -> anyhow::Result<v3::write::DirEntry<'_>> {
        Ok(v3::write::DirEntry::Dir(
            self.as_directory_tree(Strictness::default())?,
        ))
    }
}

impl AbstractVolume for MemoryVolume {
    fn read_file(
        &self,
        path: &PathSegments,
    ) -> Option<(shared_buffer::OwnedBuffer, Option<[u8; 32]>)> {
        self.node.read_file(path).map(|c| (c, None))
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.node.read_dir(path)
    }

    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        self.node.find_meta(path)
    }
}

impl WasmerPackageVolume for MemoryVolume {
    fn as_directory_tree(
        &self,
        strictness: Strictness,
    ) -> Result<webc::v3::write::Directory<'_>, anyhow::Error> {
        let res = self.node.as_directory_tree(strictness);
        res
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use v3::{
        write::Writer, Checksum, ChecksumAlgorithm, Index, IndexEntry, Signature,
        SignatureAlgorithm, Span, Tag, Timestamps,
    };
    use webc::metadata::Manifest;

    use super::*;

    fn sha256(data: impl AsRef<[u8]>) -> [u8; 32] {
        let mut state = Sha256::default();
        state.update(data.as_ref());
        state.finalize().into()
    }

    #[test]
    fn volume_metadata() -> anyhow::Result<()> {
        let file_modified = SystemTime::now();
        let file_data = String::from("Hello, world!").as_bytes().to_vec();
        let file_data_len = file_data.len();

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

        let file_metadata = volume.metadata(&PathSegments::from_str("hello.txt")?);
        assert!(file_metadata.is_some());

        let file_metadata = file_metadata.unwrap();
        assert!(file_metadata.is_file());

        let (length, timestamps) = match file_metadata {
            Metadata::File { length, timestamps } => (length, timestamps),
            _ => unreachable!(),
        };

        assert_eq!(
            timestamps.unwrap().modified(),
            file_modified.duration_since(UNIX_EPOCH)?.as_nanos() as u64
        );

        assert_eq!(length, file_data_len);

        let dir_metadata = volume.metadata(&PathSegments::from_str("/")?);
        assert!(dir_metadata.is_some());

        let dir_metadata = dir_metadata.unwrap();
        assert!(dir_metadata.is_dir());

        let timestamps = match dir_metadata {
            Metadata::Dir { timestamps } => timestamps,
            _ => unreachable!(),
        };

        assert_eq!(
            timestamps.unwrap().modified(),
            dir_modified.duration_since(UNIX_EPOCH)?.as_nanos() as u64
        );

        Ok(())
    }

    #[test]
    fn create_webc_file_from_memory() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = Manifest::default();

        let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)?
            .write_atoms(BTreeMap::new())?;

        let file_contents = "Hello, World!";
        let file = MemoryFile {
            modified: SystemTime::UNIX_EPOCH,
            data: file_contents.as_bytes().to_vec(),
        };
        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("a"), MemoryNode::File(file));

        let dir_modified = std::time::SystemTime::UNIX_EPOCH;
        let dir = MemoryDir {
            modified: dir_modified,
            nodes,
        };

        let volume = MemoryVolume { node: dir };

        writer.write_volume(
            "first",
            dbg!(WasmerPackageVolume::as_directory_tree(
                &volume,
                Strictness::Strict,
            )?),
        )?;

        let webc = writer.finish(SignatureAlgorithm::None)?;

        let mut data = vec![];
        ciborium::into_writer(&manifest, &mut data).unwrap();
        let manifest_hash: [u8; 32] = sha2::Sha256::digest(data).into();
        let manifest_section = bytes! {
            Tag::Manifest,
            manifest_hash,
            1_u64.to_le_bytes(),
            [0xa0],
        };

        let empty_hash: [u8; 32] = sha2::Sha256::new().finalize().into();

        let atoms_header_and_data = bytes! {
            // header section
            65_u64.to_le_bytes(),
            Tag::Directory,
            56_u64.to_le_bytes(),
            Timestamps::default(),
            empty_hash,
            // data section (empty)
            0_u64.to_le_bytes(),
        };

        let atoms_hash: [u8; 32] = sha2::Sha256::digest(&atoms_header_and_data).into();
        let atoms_section = bytes! {
            Tag::Atoms,
            atoms_hash,
            81_u64.to_le_bytes(),
            atoms_header_and_data,
        };

        let a_hash: [u8; 32] = sha2::Sha256::digest(file_contents).into();
        let dir_hash: [u8; 32] = sha2::Sha256::digest(a_hash).into();
        let volume_header_and_data = bytes! {
            // ==== Name ====
            5_u64.to_le_bytes(),
            "first",
            // ==== Header Section ====
            187_u64.to_le_bytes(),
            // ---- root directory ----
            Tag::Directory,
            105_u64.to_le_bytes(),
            Timestamps::default(),
            dir_hash,
            // first entry
            114_u64.to_le_bytes(),
            a_hash,
            1_u64.to_le_bytes(),
            "a",

            // ---- first item ----
            Tag::File,
            0_u64.to_le_bytes(),
            13_u64.to_le_bytes(),
            sha256("Hello, World!"),
            Timestamps::default(),

            // ==== Data Section ====
            13_u64.to_le_bytes(),
            file_contents,
        };
        let volume_hash: [u8; 32] = sha2::Sha256::digest(&volume_header_and_data).into();
        let first_volume_section = bytes! {
            Tag::Volume,
            volume_hash,
            229_u64.to_le_bytes(),
            volume_header_and_data,
        };

        let index = Index::new(
            IndexEntry::new(
                Span::new(437, 42),
                Checksum::sha256(sha256(&manifest_section[41..])),
            ),
            IndexEntry::new(
                Span::new(479, 122),
                Checksum::sha256(sha256(&atoms_section[41..])),
            ),
            [(
                "first".to_string(),
                IndexEntry::new(
                    Span::new(601, 270),
                    Checksum::sha256(sha256(&first_volume_section[41..])),
                ),
            )]
            .into_iter()
            .collect(),
            Signature::none(),
        );

        let mut serialized_index = vec![];
        ciborium::into_writer(&index, &mut serialized_index).unwrap();
        let index_section = bytes! {
            Tag::Index,
            420_u64.to_le_bytes(),
            serialized_index,
            // padding bytes to compensate for an unknown index length
            // NOTE: THIS VALUE IS COMPLETELY RANDOM AND YOU SHOULD GUESS WHAT VALUE
            // WILL WORK.
            [0_u8; 75],
        };

        assert_bytes_eq!(
            &webc,
            bytes! {
                webc::MAGIC,
                webc::Version::V3,
                index_section,
                manifest_section,
                atoms_section,
                first_volume_section,
            }
        );

        // make sure the index is accurate
        assert_bytes_eq!(&webc[index.manifest.span], manifest_section);
        assert_bytes_eq!(&webc[index.atoms.span], atoms_section);
        assert_bytes_eq!(&webc[index.volumes["first"].span], first_volume_section);

        Ok(())
    }
}
