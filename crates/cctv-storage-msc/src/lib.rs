//! Read-only Mass Storage Class (MSC) FAT32 filesystem parser.

#![forbid(unsafe_code)]

pub mod bpb;
pub mod dir;
pub mod error;
pub mod fat;
pub mod volume;

pub use bpb::BiosParameterBlock;
pub use dir::DirEntry;
pub use error::MscError;
pub use volume::Fat32Volume;
