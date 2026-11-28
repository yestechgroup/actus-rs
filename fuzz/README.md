# Fuzzing

Untrusted contract terms reach the engine through the WASM bindings (the
ACTUS Explorer). The WASM build aborts on any panic, so a panic crashes
the UI. These cargo-fuzz targets check that every input returns `Ok` or
`Err`, and that none panics or hangs.

| Target | Input |
|--------|-------|
| `json_raw` | Raw bytes split at the first NUL into terms and scenario JSON, through `validate_json`, `evaluate_json` and `evaluate_json_with_risk`. |
| `terms_structured` | Bytes driving `actus_testgen::generate`. Seed contracts for all 18 engine types are mutated towards extreme cycles, dates, amounts, rates and arrays, and get a matching risk scenario. |

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
fuzz/seed-corpus.sh
cargo +nightly fuzz run terms_structured -- -max_total_time=600 -timeout=10
cargo +nightly fuzz run json_raw -- -max_total_time=600 -timeout=10
```

The `fuzz` CI workflow runs both targets nightly for a fixed time.

## When the fuzzer finds a crash

1. Minimise the input:
   `cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-…`.
2. Add it as a regression test. API-level inputs go in
   `crates/actus-wasm/tests/regression_cases/mod.rs`, which both the native
   and the wasm-runtime tests run.
3. Fix it at the cause, returning an `EngineError`. Never catch the panic.

The same "never panic, bounded time" property runs on stable in plain CI:
`crates/actus-testgen/tests/never_panics.rs`.
