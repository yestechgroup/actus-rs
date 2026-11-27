#!/usr/bin/env bash
# Workspace line coverage with an enforced floor (#18). CI runs exactly this.
#
#   scripts/coverage.sh            # summary table, fails under the floor
#   scripts/coverage.sh --lcov     # also writes target/lcov.info
#
# Requires cargo-llvm-cov (`cargo install cargo-llvm-cov`) and the
# llvm-tools-preview rustup component.
set -euo pipefail

# Raise this as coverage improves; never lower it to make CI pass.
FLOOR_LINES=95

# Excluded from the floor, on purpose:
# - src/bin/*: the dictionary code generator and the report/site binaries.
#   The generator is guarded by the codegen drift check instead (#9).
# - actus-web/src/main.rs: the server entry point (bind + serve).
IGNORE='(/src/bin/|actus-web/src/main\.rs)'

cargo llvm-cov --workspace --ignore-filename-regex "$IGNORE" \
  --fail-under-lines "$FLOOR_LINES" --summary-only

if [[ "${1:-}" == "--lcov" ]]; then
  cargo llvm-cov report --ignore-filename-regex "$IGNORE" \
    --lcov --output-path target/lcov.info
fi
