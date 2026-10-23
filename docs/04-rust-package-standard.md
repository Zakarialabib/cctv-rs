# Rust Package Engineering Standard

A prescriptive standard for building and maintaining Rust library packages and
workspaces. Project-agnostic: it was distilled from two shipping packages of
opposite shape, and both are cited as evidence for each rule.

- **`tv-rs`** — a 6-package workspace (facade + 5 protocol crates).
- **`ezviz-openapi-rs`** — a single crate with one optional feature.

Identical copies of this file live in both repositories. Treat it as the
authority: it holds the rules. Project-specific naming detail and current status
belong in each repository's own `00-naming-conventions.md` and `03-matrix.md`
where those files exist — a project that is a single crate may not need either.

**The governing principle:** every abstraction must be *earned* — by an actual
consumer, or by a constraint the compiler enforces. Structures that exist for
symmetry, tidiness, or anticipated future need are defects. §11 lists the ones
that were removed.

---

## 1. Repository Shape: Single Crate or Workspace

Choose the shape by asking whether the split is *load-bearing*, not whether it
looks organised.

| Split is justified when | Example |
|---|---|
| Two parts need **conflicting transitive dependencies** that cannot coexist | `tv-rs`: originally `native-tls` (tizen/webos) vs `rustls` (discovery/roku); now one backend selected by feature |
| Units need **independent versions** and independent release cadences | `tv-rs` protocol crates |
| Parts have **genuinely separate dependency graphs**, so a consumer of one should not compile the others | a discovery-only consumer must not pull the WebSocket stack |

| Split is **not** justified by | Why |
|---|---|
| Tidiness, or "it's a big file" | Modules solve this. Crates add manifest, publish and version overhead. |
| One concern, one dependency set, one release | `ezviz-openapi-rs` is one crate for exactly this reason. |

Costs you accept when you split: the workspace compiles the **union** of all
dependencies (in `tv-rs` this means shipping two whole TLS stacks), every package
needs its own manifest and publish slot, and releases stop being atomic (§10). If
a single crate with feature flags achieves the same isolation, prefer that.

### Layout rules

- **Members live in their own directories at the repository root**, either as
  siblings (`tokio/`, `axum/`) or grouped under a container (`crates/<name>/`).
  Both are established:

  | Layout | Used by |
  |---|---|
  | `<name>/` at the root | tokio (`members = ["tokio", "tokio-macros", ...]`), axum (`members = ["axum", "axum-*"]`), serde, clap |
  | `crates/<name>/` | rust-analyzer, bevy, swc |

  Pick one and be consistent. Neither is more correct; the root-sibling form is
  what the most widely cited projects happen to use.
- **Never nest members under `src/`.** `src/` belongs to the root package, so a
  package at `src/<name>/` becomes a package buried inside another package's
  source tree. It also makes the root package's tarball pull in the nested crates'
  sources at publish time unless you add `include`/`exclude`.
- Declare shared metadata once in `[workspace.package]` and shared versions in
  `[workspace.dependencies]`; members inherit with `field.workspace = true`.
- List members **explicitly** rather than relying on a glob. A glob that stops
  matching (an emptied `crates/*`) fails the whole build with
  `failed to read crates\*\Cargo.toml`, which is a confusing way to learn that a
  directory moved.
- **A published workspace root must be a real package**: `[package]` *and*
  `[workspace]` in one manifest (the `tokio` pattern). A purely virtual manifest
  cannot be published, so a facade cannot be virtual.
- Every path dependency needs a `version` beside its `path`, because Cargo strips
  `path` at publish time.

---

## 2. The Facade Pattern

A facade is a package that re-exports and coordinates its siblings behind one
interface. Apply it only when it aggregates two or more packages or reduces N
dependencies to 1.

**Hard requirement: a facade must export something.** A facade with zero public
exports is pure overhead — it costs a package name, a publish slot and a version
to maintain, while giving the user nothing. If it has nothing to add, delete it.

Rules:

- The facade holds **no domain logic**. It classifies, dispatches and aggregates
  errors; the protocols stay in their own crates.
- Provide **typed escape hatches** (`as_tizen()`, `as_roku()`) instead of forcing
  a lowest-common-denominator API. Attempting to unify every vendor operation
  produces an unusable abstraction.
- Add convenience methods **only** where an equivalent exists on every backend.
  Anything else belongs on the specific client.
- **Re-export child crates as modules** (`pub use tv_rs_roku as roku;`) so users
  can always drop to the focused API without adding a dependency.

---

## 3. Crate Root Requirements

Every crate root carries, in this order:

1. A `//!` block giving the scope, the essential protocol/domain facts a reader
   needs, and **at least one runnable example**.
2. `#![forbid(unsafe_code)]` — the workspace has no `unsafe` in it, and saying so at
   the root is what keeps it that way.
3. `#![deny(missing_docs)]` is not written per crate: it is declared once in
   `[workspace.lints.rust]` and inherited through `[lints] workspace = true`, so a
   new crate cannot forget it.

Consequences to accept deliberately:

- Use `deny`, not `warn`. A missing doc becomes a build failure, so docs cannot
  rot silently. In `ezviz-openapi-rs` this cost a one-time pass of **139**
  comments; that is the correct price.
- Every crate keeps **at least one doctest**, so its example is compiled on every
  `cargo test`. A crate-level example that no longer compiles is a broken build,
  not a stale comment.
- The crate's rustdoc **is** the API documentation. Do not maintain a parallel
  `API.md` that can disagree with the code.

---

## 4. Error Contract

- One `<Crate>Error` enum per crate (`thiserror`), plus a crate-local
  `pub type Result<T>` alias.
- **No panics in library code.** `unwrap()`, `expect()`, `panic!()`,
  `unimplemented!()` and `todo!()` are unacceptable on any reachable path.
- A capability that is not implemented returns an **explicit typed variant that
  names the gap**, e.g. `Unsupported { phase, reason }`. It never panics and never
  returns a sentinel or `bool`.
- A facade aggregates child errors with `#[from]`, so `?` propagates across crate
  boundaries without conversion noise.

Rationale: a published crate must never abort a consumer's process. This is not
merely style — `unimplemented!()` in a published library is a denial-of-service
bug in someone else's binary. The original `tv-rs` stubs used `unimplemented!()`
and were replaced with typed errors before publication was attempted.

---

## 5. Testability: Pure Core, Thin Transport

Separate **wire-format logic** from **transport**. Builders, parsers, URL and
payload constructors, and key/command encoders are pure functions with no I/O.
Async clients are thin wrappers that call them.

This is what makes CI meaningful:

| Layer | How it is tested |
|---|---|
| Builders, parsers, encoders | Direct unit tests. No device, no network. |
| HTTP/WebSocket clients | A local mock server holding no credentials. |
| Real hardware | Not in CI. Exercised manually, and this limitation is stated in the docs. |

A project whose only tests require a live TV cannot be verified by anyone else.
Name tests as behavioural sentences (`identifies_each_vendor_from_its_signature`,
not `test_vendor_2`).

---

## 6. Naming

Universal rules. Project-specific detail goes in `00-naming-conventions.md`.

| Kind | Convention | Example |
|---|---|---|
| Crate / package | kebab-case | `tv-rs-roku`, `ezviz-openapi` |
| Module / file | snake_case (lowercase, hyphens only in package names) | `client.rs`, `pc_sdk.rs` |
| Type / trait | PascalCase | `RokuClient`, `EcpRestClient` |
| Trait | `<Subject><Action>` | `SsdpDiscovery`, `PowerControl` |
| Wire payload struct | `<Subject><Payload>` | `TizenChannelConnectPayload`, `SsdpMsearchResponse` |
| Function / variable | snake_case, full words | `build_magic_packet`, `client_key` |
| Constant | `UPPER_SNAKE_CASE`, prefixed by subject | `SSDP_MULTICAST_IP`, `ROKU_ECP_PORT` |
| Error | `<Crate>Error` + `Result` alias | `TizenError`, `RokuError` |

- A fixed **module vocabulary** keeps crates navigable: `lib.rs`, `error.rs`,
  `types.rs`, `payload.rs`, `protocol.rs`, `client.rs`, `auth.rs`.
- **Omit modules that do not apply.** Never create an empty module for symmetry.
  `tv-rs::discovery` and `tv-rs::power` have no `auth.rs` because SSDP and
  Wake-on-LAN are unauthenticated; the file would have been an empty abstraction.
- Established protocol acronyms are the only permitted abbreviation (`SSDP`,
  `ECP`, `SSAP`, `DIAL`, `WOL`).
- In prose, use the **package** name when discussing publishing or dependencies
  and the **directory** name when discussing the source tree. Both appearing in one
  document is intentional, not an inconsistency.

---

## 7. Documentation Set

### Structure

The index must list **every** file in `docs/` — no orphan documents. Its name is
either `docs/README.md` or `docs/00-index.md`; both are accepted and neither is
preferred, but use **one, not both** in a given repository — two indexes drift
apart.

| Slot | Role |
|---|---|
| `docs/README.md` or `docs/00-index.md` | Index; always the entry point |
| `docs/00-<topic>.md` | Conventions / standards / meta |
| `docs/01-<topic>.md` | Playbook, specs, API reference |
| `docs/02-<topic>.md` | External references, SDKs, tooling |
| `docs/03-<topic>.md` | State matrix / status |

The numbered slot is **per topic**, so several `01-` files are legitimate (a
project may have both an API reference and an integration spec). Numbering
expresses role, not sequence.

Durable, slow-changing documents stay **unnumbered** so they are not mistaken for
the operational set: `ADR-NNN-<topic>.md`, `glossary.md`.

### Content rules

- **The state matrix must be verified, not asserted.** It is the one document that
  makes claims about code, so it drifts first. `tv-rs`'s matrix previously claimed
  that `cargo check` passed and that crates had roots they did not have. Re-run
  the commands and re-count the tests before writing status.
- **ADRs carry a resolution table.** A decision record without a
  Status / Context / Decision / Resolution / Consequences structure becomes a
  stale proposal: `tv-rs`'s ADR-001 sat at "Proposed" while claiming a `LICENSE`
  file was missing that had long since been added.
- **Record rejected options with their reason.** A recommendation that was
  considered and dropped is a decision too; without a written reason it gets
  re-litigated. (`tv-rs` rejected `socket2` in favour of `tokio`'s built-in
  multicast support — that reasoning lives in `02-external-refs.md`.)
- **Do not rewrite verbatim source material.** If a document is a transcript of an
  external specification, leave the facts as given. Restating them from memory is
  how accurate docs become subtly wrong.
- **Fix documentation that contradicts the code in the same change.** Stale
  documents that assert a false state are worse than missing ones, because readers
  trust them.

---

## 8. Dependency Policy

- **License allowlist:** MIT, BSD, Apache-2.0, MPL-2.0. Direct linking is fine.
  GPL/AGPL requires process isolation, not linking.
- **Prefer a proven existing stack to re-deriving one.** `tv-rs` adopted its TLS
  and WebSocket stack from a prior-art crate rather than writing handshake code.
- **Reject additions that duplicate a capability already present.** An extra
  dependency must add capability, not convenience.
- Declare versions once in `[workspace.dependencies]`; members inherit. This
  prevents two crates quietly resolving different versions of the same package.
- **Write down the rejections.** The dependency you decided *against*, and why, is
  as valuable as the one you took.

---

## 9. CI Gates

These are the minimum for a library package:

| Gate | Command |
|---|---|
| Compiles, including tests and examples | `cargo check --all-targets` |
| No warnings (warnings are errors) | `RUSTFLAGS="-D warnings" cargo check --all-targets` |
| All tests, including doctests | `cargo test --workspace` |
| Feature combinations compile | `cargo check --all-targets --all-features` |
| Lints, where the toolchain provides them | `cargo clippy -- -D warnings` |
| License file present | publish prerequisites (§10) |
| Publish is valid before it happens | `cargo publish --dry-run` |

Two notes earned the hard way:

- **Check `--all-targets`, not just the library.** A gate that compiles only the
  lib misses broken examples and tests.
- **Do not assume the lint tool is available.** `cargo clippy` can fail to launch
  in a sandboxed or misconfigured environment. `RUSTFLAGS="-D warnings" cargo
  check` is a weaker but reliable substitute; a gate that cannot run is not a gate.
- Where a project has schema or lockfile integrity requirements, **fail the build
  rather than auto-generating** the artifact. Silent regeneration hides the drift
  that the check exists to catch.

---

## 10. Publishing

crates.io facts that constrain the design:

- **Yank, not delete.** A published version can never be removed or corrected in
  place — only a new version fixes it.
- **There is no stable `cargo publish --workspace`.** Multi-package publication is
  not atomic, so a mid-sequence failure permanently burns the versions already
  released.
- **Path dependencies are stripped at publish time**, so each must carry a
  `version` that resolves to a real registry entry.

Therefore, for a workspace:

1. Publish in **dependency order** (leaf crates first, the facade last).
2. Automate it with a script, `cargo-release` or `publish-crates`. Never do it by
   hand under time pressure.
3. **A single root-level `cargo publish` does not publish a workspace** — it
   publishes only the root package, whose path dependencies have just become
   unsatisfiable registry requirements. This exact defect was present in `tv-rs`'s
   release workflow and would have failed on first use.

Pre-release checklist:

- [ ] `LICENSE` file exists and matches the manifest's SPDX identifier
- [ ] `cargo test --workspace` green, doctests included
- [ ] No reachable `unimplemented!()`, `todo!()`, `unwrap()` in library paths
- [ ] Every crate declares `description`, `license`, `repository`, `keywords`,
      `categories`
- [ ] `cargo publish --dry-run` succeeded for **every** package
- [ ] Publish order scripted and reviewed
- [ ] Version bumped consistently across `[workspace.package]` and path deps

---

## 11. Anti-Patterns

Each of these was observed in a real repository, not invented.

| Anti-pattern | Why it is a defect | Corrected by |
|---|---|---|
| Empty facade crate exporting nothing | Costs a package, publish slot and version; benefits no consumer | Give it real behaviour or delete it |
| `unimplemented!()` in a published library | Aborts the consumer's process; a DoS bug in someone else's binary | Typed `Unsupported { phase, reason }` |
| Empty `auth.rs` kept for symmetry | Abstraction with no behaviour; misleads readers into expecting auth | Delete it; explain the asymmetry in docs |
| Modules under `src/` for packages | A package buried in another package's source tree; the root tarball pulls in the nested crates' sources unless `include`/`exclude` is set | Keep members at the root, as siblings or under `crates/` |
| Member glob that can stop matching (`crates/*`) | Moving the directory fails the whole build with `failed to read crates\*\Cargo.toml` | List members explicitly |
| Virtual workspace root when publishing | Virtual manifests cannot be published | `[package]` + `[workspace]` in one manifest |
| Single root `cargo publish` for a workspace | Publishes only the root; path deps unsatisfiable | Ordered per-package publish (§10) |
| Naming a module directory a package | Names contradict; readers cannot tell which is authoritative | Fixed vocabulary (§6) |
| Adding a dependency that duplicates a built-in | Second abstraction for no capability gain | Use the existing capability; record the rejection |
| State matrix claiming unverified builds pass | Readers trust it; it is the first document to rot | Re-run the commands before writing status |
| "Skeleton" docs after the code is complete | Docs describe a state that no longer exists | Rewrite with the code, same change |
| Re-deriving a solved TLS/WebSocket stack | Higher risk than adopting proven prior art | Adopt, then document the choice |

---

## 12. Applying This Standard to an Existing Package

1. **Verify before you document.** Run the gates in §9 and record what actually
   happens.
2. **Audit for unearned abstractions.** Anything with no consumer and no
   compiler-enforced constraint is a candidate for deletion (§11).
3. **Apply the crate-root requirements** (§3) and take the one-time doc-comment
   cost.
4. **Replace panics with typed errors** (§4). This is the highest-value change for
   anything published.
5. **Name the docs set** (§7) and make the index (`docs/README.md` or
   `docs/00-index.md`) complete.
6. **Write or repair the state matrix last**, against verified output.
7. **Fix the release path** (§10) before the first release, not during it.

## MSRV

`rust-version = "1.89"` is the **lowest version that actually resolves**, not a
target chosen for reach: `uuid` (for `CommandId`) and the ICU crates behind `idna`
declare 1.88–1.89 as their own floor. The CI `msrv` job checks the declared value,
because a `rust-version` nobody builds against is a claim, not a fact. If a
dependency raises its floor, either pin it down or raise this number — in the same
PR, never silently.

## Licensing

**One `LICENSE` at the repository root. No per-crate copies.**

Each manifest declares the licence with the SPDX identifier (`license.workspace =
true` → `MIT`). crates.io records the SPDX expression; it does not require the text
inside every tarball, and eleven copies of the same file are eleven chances to drift.

If a crate ever needs its own terms, that crate gets its own `license-file` — and the
reason goes in the ADR, not in a copy-paste.
