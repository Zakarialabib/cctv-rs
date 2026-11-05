//! Workspace automation for `cctv-rs`.
//!
//! Two commands, both reached through the alias in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask publish --dry-run
//! cargo xtask publish --version 0.3.0
//! cargo xtask hw-test --json
//! ```
//!
//! Why an xtask and not a script: the publish order is a property of the crate
//! graph — a driver must reach crates.io before the facade that re-exports it —
//! and keeping it in the workspace means `cargo` itself can check that every
//! name in the order still exists as a package.
//!
//! `scripts/publish.sh` runs the same ordered release for CI, and adds the
//! crates.io registry checks a runner needs. The order is declared in both; keep
//! the two lists in sync.

#![forbid(unsafe_code)]

mod hw;

use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::Duration;

/// Publish order. Every path dependency must reach crates.io before its
/// dependents, so the contract leads and the facade follows every crate it
/// re-exports. Keep in sync with `scripts/publish.sh` and the `members` list in
/// the root `Cargo.toml`.
const ORDER: &[&str] = &[
    "cctv-core",
    "cctv-transport",
    "cctv-rtp",
    "cctv-rtsp",
    "cctv-discovery",
    "cctv-onvif",
    "cctv-ezviz",
    "cctv-stream-uvc",
    "cctv-agent",
    "cctv-detect",
    "cctv-rag",
    "cctv-rs",
];

/// Seconds to wait between index-lag retries.
const SETTLE_RETRY: u64 = 30;

/// Seconds to wait after each upload so the crates.io index catches up.
const INDEX_SETTLE: u64 = 20;

/// crates.io allows a burst of five new crates per account, then one every ten
/// minutes. A first release of this workspace waits the limit out rather than
/// abandoning a half-finished sequence.
const RATE_LIMIT_WAIT: u64 = 600;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first() else {
        print_help();
        return Err("no command given".to_string());
    };

    match command.as_str() {
        "publish" => publish(&args[1..]),
        "hw-test" => hw::run(&args[1..]),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => {
            print_help();
            Err(format!("unknown command '{other}'"))
        }
    }
}

fn print_help() {
    println!(
        "workspace automation\n\n\
         USAGE:\n\
         \x20   cargo xtask publish [--dry-run] [--version X.Y.Z] [--token TOKEN] [--skip-tests] [--yes]\n\
         \x20   cargo xtask hw-test [--json]\n\n\
         FLAGS:\n\
         \x20   --dry-run     package and verify every crate, upload nothing\n\
         \x20   --version     bump the workspace version and every pinned path dependency\n\
         \x20   --token       crates.io token (defaults to $CRATES_IO_TOKEN)\n\
         \x20   --skip-tests  skip the test gate in the preflight\n\
         \x20   --yes         do not prompt before a real publish\n\
         \x20   --json        hw-test: emit the sweep as JSON\n\n\
         ORDER:\n\
         \x20   {}",
        ORDER.join(" -> ")
    );
}

#[derive(Debug, Default)]
struct Options {
    dry_run: bool,
    version: Option<String>,
    token: Option<String>,
    skip_tests: bool,
    assume_yes: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        token: std::env::var("CRATES_IO_TOKEN").ok(),
        ..Options::default()
    };

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dry-run" => options.dry_run = true,
            "--skip-tests" => options.skip_tests = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--version" | "--token" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("{} needs a value", args[index]))?;
                if args[index] == "--version" {
                    options.version = Some(value.clone());
                } else {
                    options.token = Some(value.clone());
                }
                index += 1;
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
        index += 1;
    }

    Ok(options)
}

fn publish(args: &[String]) -> Result<(), String> {
    let options = parse_options(args)?;

    if let Some(version) = options.version.as_deref() {
        bump_version(version)?;
        resync_lockfile()?;
    }

    preflight(&options)?;

    let mut publish_args: Vec<String> = Vec::new();
    if options.dry_run {
        // A dry run uploads nothing, so an uncommitted tree is expected while the
        // release metadata is being iterated on.
        publish_args.push("--dry-run".to_string());
        publish_args.push("--allow-dirty".to_string());
    } else {
        if !git_is_clean()? {
            return Err(
                "working tree is dirty — commit before publishing (a dirty release cannot be reproduced)"
                    .to_string(),
            );
        }
        // The token is OPTIONAL. `cargo publish` already reads cargo's own
        // credential store — `cargo login` writes `~/.cargo/credentials.toml`, and
        // `CARGO_REGISTRY_TOKEN` is the env form — so demanding one here would
        // duplicate a mechanism cargo owns, and do it in the worst place:
        // `--token <value>` puts the secret in this process's argv, where any
        // process that can list it reads it too. Pass it only when the caller
        // explicitly asked for it; otherwise let cargo find its own credentials
        // and report its own error, which names the real problem.
        if let Some(token) = options.token.clone() {
            publish_args.push("--token".to_string());
            publish_args.push(token);
        }

        if !options.assume_yes
            && !confirm(&format!("Publish {} crates to crates.io?", ORDER.len()))?
        {
            return Err("aborted by user".to_string());
        }
    }

    for (position, package) in ORDER.iter().enumerate() {
        println!(
            "\n==> publishing {package} ({}/{})",
            position + 1,
            ORDER.len()
        );
        let mut attempt = 0;
        let mut uploaded = false;
        loop {
            attempt += 1;
            let output = Command::new("cargo")
                .arg("publish")
                .arg("-p")
                .arg(package)
                .args(&publish_args)
                .output()
                .map_err(|error| format!("cannot run cargo: {error}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            print!("{stdout}{stderr}");

            if output.status.success() {
                uploaded = true;
                break;
            }

            // First publish: workspace path deps are not on crates.io yet, so a
            // dependent cannot resolve during a dry run. Expected, not a defect.
            if options.dry_run && missing_workspace_dependency(&stderr) {
                println!(
                    "warning: dry-run of {package} skipped — a workspace dependency is not on \
                     crates.io yet. Re-run after the earlier crates have been published once."
                );
                break;
            }

            // crates.io rate-limits new crates. The error names when to retry;
            // wait it out rather than abandoning a half-finished release.
            if !options.dry_run && is_rate_limited(&stderr) {
                println!(
                    "crates.io is rate-limiting {package}; waiting {RATE_LIMIT_WAIT}s before retrying"
                );
                sleep(Duration::from_secs(RATE_LIMIT_WAIT));
                uploaded = false;
                continue;
            }

            // A retry is the normal path here: the index is eventually consistent,
            // and a dependent published ten seconds ago may not be visible yet.
            if !options.dry_run && stderr.contains("no matching package named") && attempt < 5 {
                println!(
                    "index has not caught up with {package} yet; retry {attempt}/4 in {SETTLE_RETRY}s"
                );
                sleep(Duration::from_secs(SETTLE_RETRY));
                continue;
            }

            // A previous run may have uploaded this crate before failing later. That
            // is not an error: the version is immutable, so skip it and keep going.
            if !options.dry_run && already_published(&stderr) {
                println!("skip {package}: already published");
                break;
            }

            return Err(format!("cargo publish failed for {package}"));
        }

        if !options.dry_run && uploaded {
            println!("published {package}; waiting {INDEX_SETTLE}s for the index");
            sleep(Duration::from_secs(INDEX_SETTLE));
        }
    }

    if options.dry_run {
        println!("\ndry run only — nothing was uploaded");
    }
    Ok(())
}

/// Whether stderr says a workspace dependency is not in the registry yet.
///
/// Two wordings, one story: a first release has the crate absent entirely, a
/// later release has it present only at the OLD version while the new one is not
/// indexed. `ezviz-openapi` is listed too — it is published from its own
/// repository and `cctv-ezviz` cannot resolve without it.
fn missing_workspace_dependency(stderr: &str) -> bool {
    ["`cctv-", "`ezviz-openapi"].iter().any(|name| {
        stderr.contains(&format!("no matching package named {name}"))
            || stderr.contains(&format!(
                "failed to select a version for the requirement {name}"
            ))
    })
}

fn is_rate_limited(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("too many") || lower.contains("status 429") || lower.contains("rate limit")
}

fn already_published(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("already exists") || lower.contains("already uploaded")
}

fn preflight(options: &Options) -> Result<(), String> {
    println!("==> preflight");
    if !Path::new("LICENSE").exists() {
        return Err("LICENSE is missing — `cargo publish` requires it".to_string());
    }
    if !Path::new("Cargo.toml").exists() {
        return Err("Cargo.toml is missing".to_string());
    }
    assert_members_follow_the_workspace_version()?;

    gate(&["fmt", "--all", "--check"], "cargo fmt --all --check")?;
    gate(
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "cargo clippy --workspace --all-targets -- -D warnings",
    )?;

    if !options.skip_tests {
        gate(&["test", "--workspace"], "cargo test --workspace")?;
    }
    Ok(())
}

/// Fails when a workspace member hardcodes its own version.
///
/// A member that says `version = "0.3.0"` instead of `version.workspace = true`
/// does not follow a bump: the workspace package moves on, the member stays
/// behind, and the pinned `{ path = ..., version = "0.3.0" }` entry beside it
/// then cannot resolve against a member that disagrees — surfacing as
/// `failed to select a version ... candidate versions found which didn't match`,
/// which reads like a registry problem rather than a one-line manifest mistake.
fn assert_members_follow_the_workspace_version() -> Result<(), String> {
    let mut offenders = Vec::new();
    let entries =
        std::fs::read_dir("crates").map_err(|error| format!("cannot read crates dir: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read crates dir: {error}"))?;
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest)
            .map_err(|error| format!("cannot read {manifest:?}: {error}"))?;
        let hardcoded = text.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("version = \"") && !trimmed.contains("workspace")
        });
        if hardcoded {
            offenders.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    if offenders.is_empty() {
        Ok(())
    } else {
        offenders.sort();
        Err(format!(
            "these crates hardcode their version instead of `version.workspace = true`, so a \
             version bump would leave them behind: {}",
            offenders.join(", ")
        ))
    }
}

/// Re-resolves the workspace after a version bump.
///
/// `bump_version` rewrites `Cargo.toml` and leaves `Cargo.lock` pinning the OLD
/// version of every member. The new requirement beside each `path` then cannot be
/// satisfied by a lock entry that disagrees, and the very next cargo command dies
/// with `failed to select a version ... candidate versions found which didn't
/// match`. `--workspace` is the narrow form: it re-resolves the members and
/// leaves every third-party version alone, which is exactly what a bump needs.
fn resync_lockfile() -> Result<(), String> {
    gate(&["update", "--workspace"], "cargo update --workspace")
}

fn gate(args: &[&str], label: &str) -> Result<(), String> {
    println!("==> {label}");
    let status = Command::new("cargo")
        .args(args)
        .stdin(Stdio::inherit())
        .status()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed"))
    }
}

fn git_is_clean() -> Result<bool, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    Ok(output.stdout.is_empty())
}

fn confirm(question: &str) -> Result<bool, String> {
    print!("{question} [y/N] ");
    std::io::stdout()
        .flush()
        .map_err(|error| format!("cannot prompt: {error}"))?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| format!("cannot read the answer: {error}"))?;
    Ok(matches!(answer.trim(), "y" | "Y"))
}

/// Rewrites the workspace version and every pinned path-dependency version.
///
/// Only the members inherit a version (`version.workspace = true`), which
/// `assert_members_follow_the_workspace_version` guarantees, so the workspace
/// package plus the `cctv-*` pins are the whole surface. `ezviz-openapi` is
/// deliberately left alone: it is versioned in its own repository.
fn bump_version(version: &str) -> Result<(), String> {
    let valid = version
        .split('.')
        .take(3)
        .all(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        && version.matches('.').count() >= 2;
    if !valid {
        return Err(format!("--version must look like 1.2.3 (got '{version}')"));
    }

    println!("==> bumping the workspace version to {version}");
    let path = Path::new("Cargo.toml");
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    // Rewriting with a bare '\n' turns a CRLF checkout into a dirty tree, which
    // then fails the release's own clean-tree gate. Carry the file's own ending.
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };

    let mut package_bumped = false;
    let mut pins_bumped = 0_usize;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !package_bumped && trimmed.starts_with("version = \"") {
            out.push_str(&format!("version = \"{version}\""));
            package_bumped = true;
        } else if trimmed.starts_with("cctv-") && trimmed.contains("version = \"") {
            // Keep everything up to the version literal, replace the literal.
            match line.rfind("version = \"") {
                Some(at) => {
                    out.push_str(&line[..at]);
                    out.push_str(&format!("version = \"{version}\""));
                    out.push_str(" }");
                    pins_bumped += 1;
                }
                None => out.push_str(line),
            }
        } else {
            out.push_str(line);
        }
        out.push_str(eol);
    }

    if !package_bumped {
        return Err("no `[workspace.package]` version line found in Cargo.toml".to_string());
    }
    if pins_bumped == 0 {
        return Err("no path-dependency version pins found in Cargo.toml".to_string());
    }
    // Publishing the version the tree already declares must leave the tree clean.
    if out != text {
        std::fs::write(path, out).map_err(|error| error.to_string())?;
    }

    println!("version bumped ({pins_bumped} dependency pins, workspace package)");
    Ok(())
}
