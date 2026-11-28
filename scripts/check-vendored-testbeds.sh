#!/usr/bin/env bash
# Checks that every vendored official testbed under vendor/actus/tests is
# listed in [package.metadata.actus.tests_sha256] of
# crates/actus-conformance/Cargo.toml with its SHA-256, and that the listing
# names no missing file. The hashes were taken from the files as fetched from
# actusfrf/actus-tests at the pinned tests_commit (see vendor/actus/README.md).
set -euo pipefail
cd "$(dirname "$0")/.."

manifest=crates/actus-conformance/Cargo.toml
listed=$(awk '
  /^\[/ { in_section = ($0 == "[package.metadata.actus.tests_sha256]"); next }
  in_section && /=/ { gsub(/[" ]/, ""); split($0, kv, "="); print kv[2] "  " kv[1] }
' "$manifest")

if [ -z "$listed" ]; then
  echo "no [package.metadata.actus.tests_sha256] entries in $manifest" >&2
  exit 1
fi

status=0
(cd vendor/actus/tests && sha256sum --check --strict --quiet) <<<"$listed" || status=1

on_disk=$(cd vendor/actus/tests && ls actus-tests-*.json | sort)
in_manifest=$(awk '{print $2}' <<<"$listed" | sort)
if [ "$on_disk" != "$in_manifest" ]; then
  echo "vendored testbeds and $manifest disagree:" >&2
  diff <(echo "$in_manifest") <(echo "$on_disk") >&2 || true
  status=1
fi

if [ "$status" -eq 0 ]; then
  echo "$(wc -l <<<"$listed") vendored testbeds match their pinned SHA-256"
fi
exit "$status"
