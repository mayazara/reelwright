#!/usr/bin/env bash
# Runs the test suite inside a user + network namespace with no interfaces
# except an unconfigured loopback, so any network attempt fails.
# Linux only. Dependencies must already be fetched (`cargo fetch`).
set -euo pipefail
cd "$(dirname "$0")/.."
exec unshare --user --map-root-user --net -- sh -c '
  if getent hosts example.com >/dev/null 2>&1; then
    echo "test-offline: network still reachable, refusing to run" >&2
    exit 1
  fi
  RW_OFFLINE_TEST=1 cargo test --workspace --offline "$@"
' sh "$@"
