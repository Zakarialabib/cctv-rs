//! Errors for FAT32 MSC filesystem parsing.

use thiserror::Error;

/// Error taxonomy for FAT32 filesystem operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MscError {
    /// The boot sector or Bios Parameter Block (BPB) is invalid or missing signature.
    #[error("Invalid boot sector: {0}")]
    InvalidBootSector(String),

    /// The FAT table or cluster number is corrupted.
    #[error("Corrupted cluster chain at cluster {0}")]
    CorruptedClusterChain(u32),

    /// Requested file or directory path was not found.
    #[error("Path not found: {0}")]
    FileNotFound(String),

    /// Expected a directory at path, but found a file.
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Expected a file at path, but found a directory.
    #[error("Not a file: {0}")]
    NotAFile(String),

    /// An attempted read exceeded the bounds of the image slice.
    #[error("Read out of bounds")]
    ReadOutOfBounds,
}
