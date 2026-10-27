//! Read-only FAT32 Volume handle.

use crate::bpb::BiosParameterBlock;
use crate::dir::{parse_directory, DirEntry};
use crate::error::MscError;
use crate::fat::read_cluster_chain;

/// Read-only handle over a FAT32 disk image.
pub struct Fat32Volume<'a> {
    image: &'a [u8],
    bpb: BiosParameterBlock,
}

impl<'a> Fat32Volume<'a> {
    /// Open a FAT32 volume from a raw byte slice.
    pub fn open(image: &'a [u8]) -> Result<Self, MscError> {
        let bpb = BiosParameterBlock::parse(image)?;
        Ok(Self { image, bpb })
    }

    /// Access the parsed Bios Parameter Block.
    pub fn bpb(&self) -> &BiosParameterBlock {
        &self.bpb
    }

    /// List directory entries located at the specified cluster.
    pub fn list_dir(&self, cluster: u32) -> Result<Vec<DirEntry>, MscError> {
        let data = self.read_cluster_chain_bytes(cluster)?;
        parse_directory(&data)
    }

    /// List directory entries in the root directory.
    pub fn list_root(&self) -> Result<Vec<DirEntry>, MscError> {
        self.list_dir(self.bpb.root_cluster)
    }

    /// Find a directory entry given a relative path (e.g. `DCIM/100MEDIA/CAM01.JPG`).
    pub fn find_entry(&self, path: &str) -> Result<DirEntry, MscError> {
        let clean_path = path.trim_matches('/');
        if clean_path.is_empty() {
            return Ok(DirEntry {
                name: "/".to_string(),
                attributes: crate::dir::ATTR_DIRECTORY,
                first_cluster: self.bpb.root_cluster,
                file_size: 0,
            });
        }

        let segments: Vec<&str> = clean_path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_cluster = self.bpb.root_cluster;
        let mut current_entry: Option<DirEntry> = None;

        for (idx, segment) in segments.iter().enumerate() {
            let entries = self.list_dir(current_cluster)?;
            let matched = entries
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(segment))
                .ok_or_else(|| MscError::FileNotFound(path.to_string()))?;

            if idx == segments.len() - 1 {
                return Ok(matched);
            }

            if !matched.is_directory() {
                return Err(MscError::NotADirectory(segment.to_string()));
            }

            current_cluster = matched.first_cluster;
            current_entry = Some(matched);
        }

        current_entry.ok_or_else(|| MscError::FileNotFound(path.to_string()))
    }

    /// Read file bytes from volume given a path.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, MscError> {
        let entry = self.find_entry(path)?;
        if !entry.is_file() {
            return Err(MscError::NotAFile(path.to_string()));
        }

        let data = self.read_cluster_chain_bytes(entry.first_cluster)?;
        let size = entry.file_size as usize;
        if data.len() < size {
            Ok(data)
        } else {
            Ok(data[..size].to_vec())
        }
    }

    fn read_cluster_chain_bytes(&self, start_cluster: u32) -> Result<Vec<u8>, MscError> {
        let clusters = read_cluster_chain(self.image, &self.bpb, start_cluster)?;
        let cluster_bytes = self.bpb.bytes_per_cluster() as usize;
        let mut buffer = Vec::with_capacity(clusters.len() * cluster_bytes);

        for cluster in clusters {
            let offset = self.bpb.cluster_offset(cluster)? as usize;
            if offset + cluster_bytes > self.image.len() {
                return Err(MscError::ReadOutOfBounds);
            }
            buffer.extend_from_slice(&self.image[offset..offset + cluster_bytes]);
        }

        Ok(buffer)
    }
}
