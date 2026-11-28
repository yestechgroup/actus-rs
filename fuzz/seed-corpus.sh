#!/usr/bin/env bash
# Seeds fuzz/corpus/<target>/ from the vendored testbeds, the Explorer
# defaults and crates/actus-testgen/seeds/*.json. Idempotent.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run -q -p actus-testgen --bin seed_corpus -- \
  fuzz/corpus/json_raw fuzz/corpus/terms_structured
echo "json_raw:         $(ls fuzz/corpus/json_raw | wc -l) files"
echo "terms_structured: $(ls fuzz/corpus/terms_structured | wc -l) files"
