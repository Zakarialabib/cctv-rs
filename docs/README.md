# Documentation Index

Technical documentation, design decisions, and system specifications for `cctv-rs`.

## Architecture & Design

- [System Architecture](architecture.md) — System boundaries, crate topology, protocol matrix, event bus, and storage model.
- [Target Architecture](target-architecture.md) — Long-term crate evolution roadmap and core architectural decisions.
- [Engineering Standards](04-rust-package-standard.md) — Rust workspace rules, error taxonomy invariants, and CI gates.

## API & Domain Models

- [Command Catalogue](commands.md) — All 56 typed `CommandId` variants, capability guards, and result payloads.
- [Camera Taxonomy](camera-types.md) — `CameraType` hardware classification enum and fleet ordering.

## Roadmaps & Plans

- [Release Roadmap](roadmap.md) — Milestone phases, verification gates, and locked architectural decisions.
- [Execution Plan](plan.md) — Work packages, the driver consolidation track, and host application integration phases (A–E).

## Repository Metadata

- [Root README](../README.md)
- [Contributing Guide](../CONTRIBUTING.md)
- [Security Policy](../SECURITY.md)
- [Changelog](../CHANGELOG.md)
- [License](../LICENSE)
