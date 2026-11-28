# Contributing to actus-rs

Thanks for helping. This is a financial calculation engine, so correctness
comes before speed or volume. A single well-evidenced fix is worth more than
many speculative ones.

## Before you push

Run every check CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p actus-wasm --target wasm32-unknown-unknown --test wasm_runtime
scripts/coverage.sh
cargo deny check
```

- **wasm-runtime tests.** These need the `wasm32-unknown-unknown` target and
  `wasm-bindgen-test-runner` at the wasm-bindgen version in `Cargo.lock` (see
  `.cargo/config.toml`).
- **Coverage.** `scripts/coverage.sh` needs `cargo-llvm-cov` and enforces a
  95 % line-coverage floor. Never lower the floor to get green.
- **Web client.** If you touch it, also run `npm test` in
  `crates/actus-web/assets`.

CI also runs these:

- **Codegen drift check.** `cargo run -p actus-model --bin generate`
  regenerates `crates/actus-model/src/generated/` from the vendored
  dictionary, and CI fails on any diff. Never hand-edit the generated files.
- **MSRV job.** It builds and tests on the `rust-version` in the workspace
  `Cargo.toml` (currently 1.88) with `--locked`. A dependency bump that
  raises the minimum must raise `rust-version` in the same change.
- **Duplication guard.** `crates/actus-engine/tests/no_duplicated_code.rs`
  runs as part of the tests. Logic shared between engines belongs in a shared
  module (`fixed_income`, `credit_enhancement`, `common`), not in a copy.

## Proving behaviour changes: the snapshot

`cargo run -q -p actus-conformance --example snapshot` prints every event and
post-event state for:

- all official testbed cases;
- the LAM shapes replayed as LAX;
- the pinned 2048-case differential corpus.

Before any engine change:

```bash
cargo run -q -p actus-conformance --example snapshot > before.txt
```

After the change:

```bash
cargo run -q -p actus-conformance --example snapshot > after.txt
diff before.txt after.txt
```

- **A refactor must be byte-identical.**
- **A bug fix must change only the cases the bug affects.** The PR lists
  them and explains why exactly those changed.

## Test first for bugs

1. Write a regression test that reproduces the bug.
2. Run it on the unfixed code and quote the failure in the PR.
3. Fix the cause, then show the test passing.

Fix the cause, not the symptom:

- Never skip, ignore or weaken a test.
- Never add tolerances, special cases or divergence lists to get green.
- Library code returns an `EngineError`. It never panics on input that
  type-checks; don't catch panics to hide them.

## Spec questions need evidence

When the behaviour the spec requires is ambiguous, find what decides it:

- an official testbed case, found by flipping the behaviour and seeing which
  case fails (then revert the flip);
- the techspec (`vendor/actus/techspecs/actus-techspecs.tex`);
- or the reference implementation.

Record the reading in `docs/spec-interpretations.md` (`SI-001`, `SI-002`, …).
Each entry gives the spec section, the possible readings, the chosen reading
and the evidence. The oracle in `actus-conformance` is an independent
opinion, not an authority: if it agrees with the engine against the spec, fix
both.

`vendor/actus/` is normative upstream data. Never edit it.

## Pull requests

Keep PRs focused. A PR description lists:

- what changed;
- the evidence;
- the scope of snapshot output changes;
- a test plan.

A confirmed bug that the PR does not fix gets its own issue, with a minimal
reproduction, the evidence and the affected attributes or contract types.

## Licence

Contributions are licensed under the MIT licence of this repository
(`LICENSE`).
