//! Integration test for reading a canned FAT32 image read-only.

use cctv_storage_msc::Fat32Volume;

fn create_canned_fat32_image() -> Vec<u8> {
    let sector_size = 512;
    let total_sectors = 100;
    let reserved_sectors = 32;
    let sectors_per_fat = 8;
    let fat_count = 2;
    let bytes = vec![0u8; total_sectors * sector_size];

    let mut img = bytes;

    // --- Sector 0: BPB ---
    img[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]); // Jump
    img[3..11].copy_from_slice(b"MSWIN4.1"); // OEM Name
    img[11..13].copy_from_slice(&(sector_size as u16).to_le_bytes()); // Bytes per sector
    img[13] = 1; // Sectors per cluster
    img[14..16].copy_from_slice(&(reserved_sectors as u16).to_le_bytes()); // Reserved sectors
    img[16] = fat_count; // FAT count
    img[17..19].copy_from_slice(&0u16.to_le_bytes()); // Root entries (0 for FAT32)
    img[19..21].copy_from_slice(&0u16.to_le_bytes()); // Total sectors 16
    img[21] = 0xF8; // Media descriptor
    img[22..24].copy_from_slice(&0u16.to_le_bytes()); // Sectors per FAT 16
    img[32..36].copy_from_slice(&(total_sectors as u32).to_le_bytes()); // Total sectors 32
    img[36..40].copy_from_slice(&(sectors_per_fat as u32).to_le_bytes()); // Sectors per FAT 32
    img[44..48].copy_from_slice(&2u32.to_le_bytes()); // Root cluster
    img[510] = 0x55;
    img[511] = 0xAA;

    // --- FAT 1 ---
    let fat1_offset = reserved_sectors * sector_size;
    let eof_marker = 0x0F_FF_FF_F8u32.to_le_bytes();

    // Entry 0, 1 reserved
    img[fat1_offset..fat1_offset + 4].copy_from_slice(&eof_marker);
    img[fat1_offset + 4..fat1_offset + 8].copy_from_slice(&eof_marker);
    // Entry 2 (root dir)
    img[fat1_offset + 8..fat1_offset + 12].copy_from_slice(&eof_marker);
    // Entry 3 (DCIM dir)
    img[fat1_offset + 12..fat1_offset + 16].copy_from_slice(&eof_marker);
    // Entry 4 (STATUS.JSON)
    img[fat1_offset + 16..fat1_offset + 20].copy_from_slice(&eof_marker);
    // Entry 5 (FILE1.JPG)
    img[fat1_offset + 20..fat1_offset + 24].copy_from_slice(&eof_marker);

    // --- Data Region ---
    let data_offset = (reserved_sectors + fat_count as usize * sectors_per_fat) * sector_size;

    // Cluster 2: Root Directory (sector 48)
    let root_dir_offset = data_offset;

    // Entry 1: STATUS.JSON (Short name: "STATUS  JSON")
    let e1 = &mut img[root_dir_offset..root_dir_offset + 32];
    e1[0..8].copy_from_slice(b"STATUS  ");
    e1[8..11].copy_from_slice(b"JSO");
    e1[11] = 0x20; // File attribute
    e1[20..22].copy_from_slice(&0u16.to_le_bytes()); // Cluster hi
    e1[26..28].copy_from_slice(&4u16.to_le_bytes()); // Cluster lo = 4
    e1[28..32].copy_from_slice(&15u32.to_le_bytes()); // File size = 15

    // Entry 2: DCIM Directory (Short name: "DCIM    ")
    let e2 = &mut img[root_dir_offset + 32..root_dir_offset + 64];
    e2[0..8].copy_from_slice(b"DCIM    ");
    e2[8..11].copy_from_slice(b"   ");
    e2[11] = 0x10; // Directory attribute
    e2[20..22].copy_from_slice(&0u16.to_le_bytes()); // Cluster hi
    e2[26..28].copy_from_slice(&3u16.to_le_bytes()); // Cluster lo = 3
    e2[28..32].copy_from_slice(&0u32.to_le_bytes()); // File size = 0

    // Cluster 3: DCIM Directory (sector 49)
    let dcim_dir_offset = data_offset + sector_size;
    let sub_e1 = &mut img[dcim_dir_offset..dcim_dir_offset + 32];
    sub_e1[0..8].copy_from_slice(b"FILE1   ");
    sub_e1[8..11].copy_from_slice(b"JPG");
    sub_e1[11] = 0x20; // File attribute
    sub_e1[20..22].copy_from_slice(&0u16.to_le_bytes());
    sub_e1[26..28].copy_from_slice(&5u16.to_le_bytes()); // Cluster lo = 5
    sub_e1[28..32].copy_from_slice(&12u32.to_le_bytes()); // File size = 12

    // Cluster 4: STATUS.JSON data (sector 50)
    let status_file_offset = data_offset + 2 * sector_size;
    img[status_file_offset..status_file_offset + 15].copy_from_slice(b"{\"status\":\"ok\"}");

    // Cluster 5: FILE1.JPG data (sector 51)
    let jpg_file_offset = data_offset + 3 * sector_size;
    img[jpg_file_offset..jpg_file_offset + 12].copy_from_slice(b"JPEG-DATA-12");

    img
}

#[test]
fn test_parse_canned_fat32_image_read_only() {
    let img_bytes = create_canned_fat32_image();
    let volume = Fat32Volume::open(&img_bytes).expect("Volume should open");

    assert_eq!(volume.bpb().bytes_per_sector, 512);
    assert_eq!(volume.bpb().sectors_per_cluster, 1);
    assert_eq!(volume.bpb().root_cluster, 2);

    let root_entries = volume.list_root().expect("Root directory entries");
    assert_eq!(root_entries.len(), 2);
    assert_eq!(root_entries[0].name, "STATUS.JSO");
    assert_eq!(root_entries[1].name, "DCIM");
    assert!(root_entries[1].is_directory());

    let status_data = volume.read_file("STATUS.JSO").expect("Read STATUS.JSO");
    assert_eq!(status_data, b"{\"status\":\"ok\"}");

    let jpg_data = volume.read_file("DCIM/FILE1.JPG").expect("Read FILE1.JPG");
    assert_eq!(jpg_data, b"JPEG-DATA-12");
}
