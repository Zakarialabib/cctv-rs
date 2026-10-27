# cctv-adapter-actioncam

Action Camera adapter for cctv-rs providing passive mDNS TXT record discovery and read-only status endpoint inspection for cameras such as the Insta360 X4.

## Safety & Principles

- **Passive Operation:** Proves zero command bytes or mutating operations (such as shutter triggers or format operations) are transmitted during discovery and health inspection.
- **`#![forbid(unsafe_code)]`:** Enforced across the entire crate.
