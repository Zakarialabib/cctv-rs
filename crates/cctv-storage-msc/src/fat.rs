//! FAT32 File Allocation Table cluster chain reader.

use crate::bpb::BiosParameterBlock;
use crate::error::MscError;

/// Reads cluster chains from the FAT table.
pub fn read_cluster_chain(
    image: &[u8],
    bpb: &BiosParameterBlock,
    start_cluster: u32,
) -> Result<Vec<u32>, MscError> {
    let mut chain = Vec::new();
    let mut current = start_cluster;
    let fat_offset = bpb.fat1_offset() as usize;

    while (2..0x0F_FF_FF_F7).contains(&current) {
        chain.push(current);

        let entry_offset = fat_offset + (current as usize * 4);
        if entry_offset + 4 > image.len() {
            return Err(MscError::ReadOutOfBounds);
        }

        let next_raw =
            u32::from_le_bytes(image[entry_offset..entry_offset + 4].try_into().unwrap());
        let next = next_raw & 0x0F_FF_FF_FF;

        if next >= 0x0F_FF_FF_F8 {
            break; // End of cluster chain
        }

        if chain.contains(&next) {
            return Err(MscError::CorruptedClusterChain(next)); // Cycle detected
        }

        current = next;
    }

    Ok(chain)
}
