//! FAT32 directory entry parsing.

use crate::error::MscError;

/// Attribute mask for directory.
pub const ATTR_DIRECTORY: u8 = 0x10;
/// Attribute mask for Volume ID.
pub const ATTR_VOLUME_ID: u8 = 0x08;
/// Attribute mask for LFN entries.
pub const ATTR_LFN: u8 = 0x0F;

/// Parsed FAT32 directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// File or directory name.
    pub name: String,
    /// Attribute flags byte.
    pub attributes: u8,
    /// First cluster of file/directory data.
    pub first_cluster: u32,
    /// Byte size of file (0 for directories).
    pub file_size: u32,
}

impl DirEntry {
    /// Check whether this entry represents a directory.
    pub fn is_directory(&self) -> bool {
        (self.attributes & ATTR_DIRECTORY) != 0
    }

    /// Check whether this entry represents a regular file.
    pub fn is_file(&self) -> bool {
        !self.is_directory() && (self.attributes & ATTR_VOLUME_ID) == 0
    }
}

/// Parse directory entries from cluster buffer.
pub fn parse_directory(data: &[u8]) -> Result<Vec<DirEntry>, MscError> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    let mut lfn_parts: Vec<(u8, String)> = Vec::new();

    while cursor + 32 <= data.len() {
        let raw = &data[cursor..cursor + 32];
        cursor += 32;

        let first_byte = raw[0];
        if first_byte == 0x00 {
            break; // End of directory
        }
        if first_byte == 0xE5 {
            lfn_parts.clear();
            continue; // Deleted entry
        }

        let attr = raw[11];
        if attr == ATTR_LFN {
            // Parse LFN fragment
            let sequence = raw[0] & 0x1F;
            let mut utf16_units = Vec::new();

            // Characters 1-5 (offsets 1..11)
            append_utf16_pair(&mut utf16_units, &raw[1..11]);
            // Characters 6-11 (offsets 14..26)
            append_utf16_pair(&mut utf16_units, &raw[14..26]);
            // Characters 12-13 (offsets 28..32)
            append_utf16_pair(&mut utf16_units, &raw[28..32]);

            let fragment = String::from_utf16_lossy(&utf16_units)
                .trim_matches('\0')
                .to_string();
            lfn_parts.push((sequence, fragment));
            continue;
        }

        if (attr & ATTR_VOLUME_ID) != 0 && (attr & ATTR_DIRECTORY) == 0 {
            lfn_parts.clear();
            continue; // Skip Volume ID
        }

        // Determine name (LFN if present, else 8.3 short name)
        let name = if !lfn_parts.is_empty() {
            lfn_parts.sort_by_key(|(seq, _)| *seq);
            let combined: String = lfn_parts.iter().map(|(_, frag)| frag.as_str()).collect();
            lfn_parts.clear();
            combined
        } else {
            format_short_name(&raw[0..8], &raw[8..11])
        };

        if name == "." || name == ".." {
            continue;
        }

        let cluster_hi = u16::from_le_bytes(raw[20..22].try_into().unwrap()) as u32;
        let cluster_lo = u16::from_le_bytes(raw[26..28].try_into().unwrap()) as u32;
        let first_cluster = (cluster_hi << 16) | cluster_lo;
        let file_size = u32::from_le_bytes(raw[28..32].try_into().unwrap());

        entries.push(DirEntry {
            name,
            attributes: attr,
            first_cluster,
            file_size,
        });
    }

    Ok(entries)
}

fn append_utf16_pair(out: &mut Vec<u16>, slice: &[u8]) {
    for chunk in slice.chunks(2) {
        if chunk.len() < 2 {
            break;
        }
        let code = u16::from_le_bytes(chunk.try_into().unwrap());
        if code == 0x0000 || code == 0xFFFF {
            break;
        }
        out.push(code);
    }
}

fn format_short_name(name_bytes: &[u8], ext_bytes: &[u8]) -> String {
    let name = String::from_utf8_lossy(name_bytes).trim().to_string();
    let ext = String::from_utf8_lossy(ext_bytes).trim().to_string();

    if ext.is_empty() {
        name
    } else {
        format!("{name}.{ext}")
    }
}
