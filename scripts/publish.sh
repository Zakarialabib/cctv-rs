#!/usr/bin/env bash
#
# Ordered publication for the cctv-rs workspace.
#
# crates.io has no `cargo publish --workspace`, and a release is not atomic: a
# mid-sequence failure permanently burns the versions already uploaded. The order
# below is therefore dependency order — leaf crates first, the facade last.
#
# Usage:
#   ./scripts/publish.sh --dry-run    # package and verify every crate, upload nothing
#   ./scripts/publish.sh --publish    # real release; requires CRATES_IO_TOKEN
#
# Three behaviours are deliberate, and all three come from crates.io:
#
#   * A dry run of a dependent crate *cannot* resolve its workspace dependency
#     before that dependency has been published once, because `cargo publish`
#     strips `path` and looks the version up in the registry index. That failure
#     is reported as a warning, not an error — the manifest, the metadata and the
#     file list have already been validated by then.
#   * A real publish of a dependent crate can fail the same way for a few seconds
#     after its dependency was uploaded, while the index catches up. That is a
#     retry, not an error.
#   * crates.io rate-limits *new* crates to a burst of 5 per account and then 1
#     every 10 minutes. A first release of this workspace is seven new crates
#     (eight with `ezviz-openapi`), so the tail of the order waits. The registry's
#     error names the time to retry; this script waits and retries rather than
#     failing a release half way through.
#
# NOTE `ezviz-openapi` is NOT in this list: it is published from its own
# repository (c:\laragon\www\ezviz-openapi-rs) and must already exist on
# crates.io before `cctv-ezviz` is published, because `cargo publish` strips the
# `git` key from a `{ git, version }` dependency and resolves the version from the
# registry. Publish it first, then run this script.
#
# NOTE the facade is `cctv-rs`, not `cctv`: `cctv` is taken on crates.io by an
# unrelated project.

set -euo pipefail

ORDER=(
    cctv-core        # no workspace deps
    cctv-transport   # no workspace deps
    cctv-onvif       # -> cctv-core, cctv-transport
    cctv-discovery   # -> cctv-core
    cctv-ezviz       # -> ezviz-openapi (published separately: must land first)
    cctv-agent       # -> cctv-core
    cctv-rs          # facade: depends on all of the above; `cctv` itself is taken
)

# The one dependency that does not live in this repository.
EXTERNAL_DEP=ezviz-openapi

# Seconds to wait for the index to catch up: between retries, and after each
# successful upload before moving on to its dependents.
SETTLE_RETRY=30
INDEX_SETTLE=20

# crates.io allows a burst of 5 new crates per account, then 1 every 10 minutes.
RATE_LIMIT_WAIT=600
MAX_ATTEMPTS=5
MAX_RATE_LIMIT_ATTEMPTS=12

MODE="${1:---dry-run}"

case "$MODE" in
    --dry-run)
        PUBLISH_ARGS=(--dry-run --allow-dirty)
        ;;
    --publish)
        # A token can arrive two ways: the environment (handed to cargo through
        # CARGO_REGISTRY_TOKEN, never argv — `--token` is visible in the process
        # list), or `cargo login`'s stored credentials. Accept either.
        if [ -n "${CRATES_IO_TOKEN:-}" ]; then
            export CARGO_REGISTRY_TOKEN="$CRATES_IO_TOKEN"
        elif [ -f "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" ] \
            || [ -f "${CARGO_HOME:-$HOME/.cargo}/credentials" ]; then
            echo "==> no CRATES_IO_TOKEN in the environment; using cargo's stored credentials"
        else
            echo "error: no crates.io token. Either export CRATES_IO_TOKEN, or run" >&2
            echo "       \`cargo login\` once to store one in ~/.cargo/credentials.toml." >&2
            exit 1
        fi
        if [ -n "$(git status --porcelain)" ]; then
            echo "error: working tree is dirty; commit or stash before publishing" >&2
            exit 1
        fi
        PUBLISH_ARGS=()
        ;;
    *)
        echo "usage: $0 [--dry-run|--publish]" >&2
        exit 2
        ;;
esac

if [ ! -f LICENSE ]; then
    echo "error: LICENSE file missing — cargo publish requires it" >&2
    exit 1
fi

# Is a crate already on crates.io? `cargo search` is the only query available
# without another tool; if it fails (offline, rate-limited) the answer is
# "unknown", which is treated as "not published" so the publish path still runs.
crate_published() {
    cargo search "$1" --limit 20 2>/dev/null | grep -qE "^$1 = "
}

if [ "$MODE" = "--publish" ]; then
    if crate_published "$EXTERNAL_DEP"; then
        echo "==> $EXTERNAL_DEP is on crates.io"
    else
        echo "error: $EXTERNAL_DEP is not on crates.io, and cctv-ezviz depends on it." >&2
        echo "       Publish it from its own repository first:" >&2
        echo "         cd ../ezviz-openapi-rs && cargo publish --token \"\$CRATES_IO_TOKEN\"" >&2
        echo "       (a first release of this workspace is 7 new crates; crates.io" >&2
        echo "        allows a burst of 5, so plan for the rate limit either way)" >&2
        exit 1
    fi
    echo "==> publishing ${#ORDER[@]} crates; crates.io allows a burst of 5 new"
    echo "    crates per account, then 1 every ${RATE_LIMIT_WAIT}s — the tail waits."
fi

skipped_list=""
skipped_count=0

for crate in "${ORDER[@]}"; do
    echo "==> $MODE $crate"
    attempt=1
    rate_attempt=1
    while :; do
        set +e
        output="$(cargo publish -p "$crate" "${PUBLISH_ARGS[@]}" 2>&1)"
        status=$?
        set -e
        printf '%s\n' "$output"

        [ "$status" -eq 0 ] && break

        # crates.io rate-limits new crates. The error names when to retry; wait
        # and try again rather than abandoning a half-finished release.
        if printf '%s' "$output" | grep -qiE 'too many|status 429|rate limit'; then
            if [ "$MODE" = "--publish" ] && [ "$rate_attempt" -lt "$MAX_RATE_LIMIT_ATTEMPTS" ]; then
                echo "==> crates.io rate limit reached for $crate; waiting ${RATE_LIMIT_WAIT}s (attempt $rate_attempt/$((MAX_RATE_LIMIT_ATTEMPTS - 1)))"
                sleep "$RATE_LIMIT_WAIT"
                rate_attempt=$((rate_attempt + 1))
                continue
            fi
            echo "error: $crate is still rate-limited after $rate_attempt attempts" >&2
            exit 1
        fi

        # The dependency is not on crates.io (yet). During a dry run this is the
        # expected first-release state; during a real publish it is index lag.
        if printf '%s' "$output" | grep -q 'no matching package named `\(cctv\|ezviz-openapi\)'; then
            if [ "$MODE" = "--dry-run" ]; then
                echo "warning: $crate not fully verified — a workspace dependency is not on" \
                     "crates.io yet. Re-run after the earlier crates have been published."
                skipped_list="$skipped_list $crate"
                skipped_count=$((skipped_count + 1))
                break
            fi
            if [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
                echo "==> index has not caught up with $crate; retry $attempt/$((MAX_ATTEMPTS - 1)) in ${SETTLE_RETRY}s"
                sleep "$SETTLE_RETRY"
                attempt=$((attempt + 1))
                continue
            fi
        fi

        # A previous run may have uploaded this crate before failing later on. The
        # version is immutable, so it can never be re-uploaded — skip and continue.
        if [ "$MODE" = "--publish" ] \
            && printf '%s' "$output" | grep -qE 'already (exists|uploaded)'; then
            echo "==> $crate is already published; skipping"
            break
        fi

        echo "error: publishing $crate failed" >&2
        exit 1
    done

    if [ "$MODE" = "--publish" ]; then
        echo "==> published $crate; waiting ${INDEX_SETTLE}s for the index"
        sleep "$INDEX_SETTLE"
    fi
done

if [ "$MODE" = "--dry-run" ]; then
    echo "==> dry run complete (${#ORDER[@]} crates, $skipped_count awaiting a first publish)"
    if [ "$skipped_count" -gt 0 ]; then
        echo "    not fully verified:${skipped_list}"
    fi
    echo "    nothing was uploaded"
else
    echo "==> publish complete (${#ORDER[@]} crates)"
fi
