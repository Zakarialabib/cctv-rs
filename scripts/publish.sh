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
# Two behaviours are deliberate, and both come from crates.io being eventually
# consistent:
#
#   * A dry run of a dependent crate *cannot* resolve its workspace dependency
#     before that dependency has been published once, because `cargo publish`
#     strips `path` and looks the version up in the registry index. That failure
#     is reported as a warning, not an error — the manifest, the metadata and the
#     file list have already been validated by then.
#   * A real publish of a dependent crate can fail the same way for a few seconds
#     after its dependency was uploaded, while the index catches up. That is a
#     retry, not an error.
#
# NOTE `ezviz-openapi` is NOT in this list: it is published from
# c:\laragon\www\ezviz-openapi-rs and must already exist on crates.io before
# `cctv-ezviz` is published. It has not been published yet, so `cctv-ezviz` is
# the one crate in the order that cannot be released from this repository alone.
#
# NOTE the facade is `cctv-rs`, not `cctv`: `cctv` is taken on crates.io by an
# unrelated project.

set -euo pipefail

ORDER=(
    cctv-core        # no workspace deps
    cctv-transport   # no workspace deps
    cctv-discovery   # -> cctv-core
    cctv-ezviz       # -> ezviz-openapi (not published yet: must land first)
    cctv-agent       # -> cctv-core
    cctv-rs          # facade: depends on all of the above; `cctv` itself is taken
)

# Seconds to wait for the index to catch up: between retries, and after each
# successful upload before moving on to its dependents.
SETTLE_RETRY=30
INDEX_SETTLE=20
MAX_ATTEMPTS=5

MODE="${1:---dry-run}"

case "$MODE" in
    --dry-run)
        PUBLISH_ARGS=(--dry-run --allow-dirty)
        ;;
    --publish)
        if [ -z "${CRATES_IO_TOKEN:-}" ]; then
            echo "error: CRATES_IO_TOKEN is not set" >&2
            exit 1
        fi
        if [ -n "$(git status --porcelain)" ]; then
            echo "error: working tree is dirty; commit or stash before publishing" >&2
            exit 1
        fi
        PUBLISH_ARGS=(--token "$CRATES_IO_TOKEN")
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

skipped_list=""
skipped_count=0

for crate in "${ORDER[@]}"; do
    echo "==> $MODE $crate"
    attempt=1
    while :; do
        set +e
        output="$(cargo publish -p "$crate" "${PUBLISH_ARGS[@]}" 2>&1)"
        status=$?
        set -e
        printf '%s\n' "$output"

        [ "$status" -eq 0 ] && break

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
