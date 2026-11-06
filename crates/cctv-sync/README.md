# cctv-sync

Cross-device state synchronization crate providing conflict-free replicated data types (CRDTs)
for the CCTV trust store and camera device registry.

## Architecture

- **`LwwRegister<T>`**: Last-Write-Wins register parameterized by logical or physical wall timestamps and unique node identifiers for deterministic tie-breaking.
- **`OrSet<T>`**: Observed-Remove Set (state-based CRDT) supporting concurrent additions and removals without lost updates or resurrected tombstones.
- **`SyncTrustStore`**: Replicated trust store holding paired device peers (`TrustedPeer`), public keys, pairing ceremonies, and revocations.
- **`SyncDeviceRegistry`**: Replicated camera registry holding `CameraConfig`, connection profiles, and metadata across workspaces and edge nodes.
