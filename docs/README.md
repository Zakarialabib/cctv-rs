# Documentation

Every document in `docs/`, and what it is for. Start here.

## Where to go

| If you want to…                                                        | Read                                        |
| ---------------------------------------------------------------------- | ------------------------------------------- |
| understand what `cctv-rs` is and how the crates fit together           | [architecture.md](architecture.md)          |
| see the command inventory (`CommandId`)                                | [commands.md](commands.md)                  |
| understand `CameraType` and how capabilities are modelled              | [camera-types.md](camera-types.md)          |
| know what is built, what is next, and what is deliberately deferred    | [roadmap.md](roadmap.md)                    |
| know where the workspace is headed, and the delta from today           | [target-architecture.md](target-architecture.md) |
| follow the house conventions — layout, error contract, CI gates        | [04-rust-package-standard.md](04-rust-package-standard.md) |

## Contents

| Document                                                       | Role                                                                                                                                                          |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [architecture.md](architecture.md)                             | The system: the camera use surface, the crate map and dependency direction, the protocol matrix, the event taxonomy, the storage model and the integration seam. |
| [commands.md](commands.md)                                     | The `CommandId` catalogue — the *what* a camera is asked to do — plus the result payloads.                                                                     |
| [camera-types.md](camera-types.md)                             | The `CameraType` taxonomy, its stable order, and the rule that capabilities are not types.                                                                     |
| [roadmap.md](roadmap.md)                                       | Implementation phases with their gates, the locked design decisions, the open items and the acceptance criteria.                                                |
| [target-architecture.md](target-architecture.md)               | The v0.2+ crate map, the measured delta from what is built, the decisions it raises, and the Simple-Signage integration seam. **Aspirational** — `architecture.md` is the truth. |
| [04-rust-package-standard.md](04-rust-package-standard.md)     | The house engineering standard. **Authority** for repository shape, the error contract, the CI gates and the release path.                                      |
| [README.md](README.md)                                         | This file.                                                                                                                                                    |

## Repository root

[`README.md`](../README.md) · [`CONTRIBUTING.md`](../CONTRIBUTING.md) ·
[`SECURITY.md`](../SECURITY.md) · [`CHANGELOG.md`](../CHANGELOG.md) ·
[`LICENSE`](../LICENSE)
