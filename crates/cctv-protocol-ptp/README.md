# cctv-protocol-ptp

Pure Rust implementation of Picture Transfer Protocol (PTP / ISO 15740 / PTP-IP) PDU framing, operation codecs, session management, and mock responder suite for camera fleet integration.

## Architecture

- **`codes`**: Operation codes, response codes, and packet types defined by ISO 15740 and PTP-IP specifications.
- **`pdu`**: Zero-copy PDU serialization and deserialization for command, data, and response phases.
- **`client`**: Async PTP-IP client for establishing sessions, fetching device information, enumerating storage IDs, and reading object handles.
- **`mock`**: Lightweight mock PTP responder server for round-trip testing and protocol conformance verification without physical camera hardware.

## Safety & Principles

- `#![forbid(unsafe_code)]` enforced across the entire crate.
- Zero panics: All malformed bytes, unexpected sequence numbers, or transport disconnections produce typed `PtpError` values.
