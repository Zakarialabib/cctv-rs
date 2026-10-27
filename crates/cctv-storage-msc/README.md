# cctv-storage-msc

Pure Rust, read-only Mass Storage Class (MSC) FAT32 filesystem parser for camera media inspection and storage indexing.

## Features

- **Read-Only Guarantee:** Implements strictly non-mutating parsing of Master Boot Record / Volume Boot Record (BPB), FAT32 File Allocation Table, directory structures (8.3 & LFN), and cluster chains.
- **Pure Byte Parsing:** Zero C/OS kernel dependencies, operating directly on slice references (`&[u8]`).
- **Zero Panic Safety:** `#![forbid(unsafe_code)]` enforced across the entire crate.

## Usage

```rust,ignore
use cctv_storage_msc::Fat32Volume;

let fat32_bytes: &[u8] = include_bytes!("../fixtures/canned_fat32.img");
let volume = Fat32Volume::open(fat32_bytes)?;

let entries = volume.list_root()?;
let file_data = volume.read_file("DCIM/100MEDIA/CAM0001.JPG")?;
```
