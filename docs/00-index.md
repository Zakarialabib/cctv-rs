# cctv-rs Documentation

> The numbered, single-source documentation set for the `cctv-rs` workspace.

Each document owns **one subject**. Where two documents could overlap, one links to
the other rather than restating it: the engineering standard
([02](02-rust-package-standard.md)) is the authority for *process*, the execution
plan ([07](07-plan.md)) for *scope and schedule*, and the architecture
([01](01-architecture.md)) for *structure*.

---

## Reading order

| # | Document | Subject |
| --- | --- | --- |
| [00](00-index.md) | **Index** | This map: reading order, authority, conventions. |
| [01](01-architecture.md) | **Architecture** | Crate topology, interface surface, protocol matrix, invariants, contract boundaries. |
| [02](02-rust-package-standard.md) | **Engineering Standard** | Workspace rules, error/panic policy, security, CI gates. *Authority for process.* |
| [03](03-commands.md) | **Command Catalogue** | Every `CommandId` variant, by domain group, and the result payloads. |
| [04](04-camera-types.md) | **Camera Taxonomy** | `CameraType` classification and fleet ordering. |
| [05](05-quic-tuning.md) | **QUIC Tuning** | Congestion control and stream-window profile for live video. |
| [06](06-roadmap.md) | **Roadmap** | Milestone phases, release history, locked decisions. |
| [07](07-plan.md) | **Execution Plan** | Work packages, consolidation ledger, milestones, gates. *Authority for scope.* |

---

## Conventions

- **Numbered, ordered.** Documents run `00`–`07`. Numbers are stable: a new
  subject takes the next free number, and existing numbers are not reused.
- **One subject per file.** Overlap between documents is resolved by linking,
  never by duplication.
- **Authority.** Process → [02](02-rust-package-standard.md); scope and schedule
  → [07](07-plan.md); structure → [01](01-architecture.md).
- **Links.** All links are relative. Every document closes with a *See also*
  block and the license & security footer.

---

## Repository metadata

- [Root README](../README.md)
- [Contributing Guide](../CONTRIBUTING.md)
- [Security Policy](../SECURITY.md)
- [Changelog](../CHANGELOG.md)
- [License](../LICENSE)

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
