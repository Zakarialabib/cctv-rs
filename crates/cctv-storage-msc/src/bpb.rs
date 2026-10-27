//! FAT32 Bios Parameter Block (BPB) parser.

use crate::error::MscError;

/// Parsed FAT32 Bios Parameter Block (VBR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosParameterBlock {
    /// Bytes per sector (typically 512).
    pub bytes_per_sector: u16,
    /// Sectors per cluster (typically 1, 2, 4, 8, 16, 32, 64).
    pub sectors_per_cluster: u8,
    /// Number of reserved sectors before the first FAT.
    pub reserved_sectors: u16,
    /// Number of File Allocation Tables (typically 2).
    pub fat_count: u8,
    /// Total sectors on volume.
    pub total_sectors: u32,
    /// Sectors occupied by each FAT32 table.
    pub sectors_per_fat: u32,
    /// Cluster number where root directory starts (typically 2).
    pub root_cluster: u32,
}

impl BiosParameterBlock {
    /// Parse FAT32 BPB from volume sector 0.
    pub fn parse(sector0: &[u8]) -> Result<Self, MscError> {
        if sector0.len() < 512 {
            return Err(MscError::ReadOutOfBounds);
        }

        // Check boot signature 0x55, 0xAA at offset 510
        if sector0[510] != 0x55 || sector0[511] != 0xAA {
            return Err(MscError::InvalidBootSector(
                "Missing 0x55AA boot signature".to_string(),
            ));
        }

        let bytes_per_sector = u16::from_le_bytes(sector0[11..13].try_into().unwrap());
        let sectors_per_cluster = sector0[13];
        let reserved_sectors = u16::from_le_bytes(sector0[14..16].try_into().unwrap());
        let fat_count = sector0[16];

        let total_sectors_16 = u16::from_le_bytes(sector0[19..21].try_into().unwrap()) as u32;
        let total_sectors_32 = u32::from_le_bytes(sector0[32..36].try_into().unwrap());
        let total_sectors = if total_sectors_16 != 0 {
            total_sectors_16
        } else {
            total_sectors_32
        };

        let sectors_per_fat_16 = u16::from_le_bytes(sector0[22..24].try_into().unwrap()) as u32;
        let sectors_per_fat_32 = u32::from_le_bytes(sector0[36..40].try_into().unwrap());
        let sectors_per_fat = if sectors_per_fat_16 != 0 {
            sectors_per_fat_16
        } else {
            sectors_per_fat_32
        };

        let root_cluster = u32::from_le_bytes(sector0[44..48].try_into().unwrap());

        if bytes_per_sector == 0 || sectors_per_cluster == 0 || fat_count == 0 {
            return Err(MscError::InvalidBootSector(
                "Invalid BPB parameters".to_string(),
            ));
        }

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            total_sectors,
            sectors_per_fat,
            root_cluster,
        })
    }

    /// Calculate bytes per cluster.
    pub fn bytes_per_cluster(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    /// Byte offset where FAT 1 starts.
    pub fn fat1_offset(&self) -> u64 {
        self.reserved_sectors as u64 * self.bytes_per_sector as u64
    }

    /// Byte offset where cluster data region starts.
    pub fn data_start_offset(&self) -> u64 {
        let fat_sectors = self.fat_count as u64 * self.sectors_per_fat as u64;
        (self.reserved_sectors as u64 + fat_sectors) * self.bytes_per_sector as u64
    }

    /// Calculate byte offset for a given cluster number (clusters start at 2).
    pub fn cluster_offset(&self, cluster: u32) -> Result<u64, MscError> {
        if cluster < 2 {
            return Err(MscError::CorruptedClusterChain(cluster));
        }
        let cluster_index = (cluster - 2) as u64;
        let offset = self.data_start_offset() + cluster_index * self.bytes_per_cluster() as u64;
        Ok(offset)
    }
}
