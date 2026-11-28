# actus-rs

ACTUS in Rust: the [ACTUS](https://www.actusfrf.org/) Data Standard vocabulary,
contract terms, and the Algorithmic Standard execution engine, plus a
differential conformance harness against the official actusfrf testbeds.

| Crate | Purpose |
|-------|---------|
| `actus-model` | Contract terms vocabulary generated from the ACTUS dictionary; vendored testbed vectors |
| `actus-engine` | Schedule generation, state variables, event sequencing, day counts; contract types PAM, ANN, LAM, LAX, NAM, CEC, CLM, CSH, UMP, STK, COM, FXOUT, SWPPV, SWAPS, CAPFL, OPTNS, FUTUR, CEG |
| `actus-conformance` | Deterministic seeded corpus, oracle, comparator; differential tests vs the actusfrf testbeds |
| `actus-wasm` | wasm-bindgen bindings: JSON terms → evaluated event schedules, applicability metadata and validation for browsers |
| `actus-web` | "ACTUS Explorer" web visualizer: WebUI (Microsoft SSR framework) frontend + axum server, driven by `actus-wasm` |

Design properties: **pure** (no I/O, no clock, no randomness — results derive
from inputs alone), `rust_decimal::Decimal` money, `chrono` timestamps,
hermetic tests (the conformance corpus is seeded in-memory and pinned by hash;
the actusfrf dictionary/techspecs/tests are vendored under `vendor/actus/`).

## Status: pre-1.0, under active development

Engines exist for 18 of the 21 dictionary contract types (BCS, CDSWP and
MAR have none yet), and `actus_engine::evaluate` dispatches to all of them.
How thoroughly each type is checked varies:

- PAM, LAM, NAM and ANN pass their official actusfrf testbeds and agree
  with an independent oracle on a 2048-case differential corpus.
- LAX is gated on the LAM testbed and corpus (a LAX without arrays is a LAM).
- CSH, CLM, SWAPS and CEC pass their official testbeds.
- The remaining types have hand-computed unit tests only.

CI runs all of it on every push, with a 95% line coverage floor. The plan
to v1.0, including formal verification and the web explorer, is in
[`ROADMAP.md`](ROADMAP.md). Who v1.0 is for is an open decision
(yestechgroup/actus-rs#25).

History note: these crates originated in [onboarding-os](https://github.com/yestechgroup/onboarding-os)
(see its ADR 0001); that repo's `financial-domain` and `onboarding-extensions`
consume `actus-model`/`actus-engine` by git rev pin.

## Build & test

```bash
cargo test --workspace        # includes the differential conformance suite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Web visualizer (ACTUS Explorer)

Issue [#1](https://github.com/yestechgroup/actus-rs/issues/1): a browser
explorer for contract schedules and the ACTUS applicability rules, built on
[WebUI](https://microsoft.github.io/webui/) (server-side rendered templates,
hydrated islands) with the contract engine compiled to WASM
(`crates/actus-wasm`). The server and the browser share one evaluation code
path, so server-rendered and client-recomputed schedules are identical by
construction.

```bash
cd crates/actus-web/assets && npm install && cd ../../..
wasm-pack build crates/actus-wasm --target web --out-dir pkg
cargo run -p actus-web --bin build-site
cargo run -p actus-web --bin actus-web        # http://127.0.0.1:8080
```

See `crates/actus-web/README.md` for details. Engines for the 18 v1 contract
types are implemented against `vendor/actus/docs/paper.md`; the eight types
with official actusfrf testbeds are conformance-gated, the rest carry
hand-computed golden tests.

## License

MIT, see [`LICENSE`](LICENSE). The vendored ACTUS dictionary, techspecs, and
testbed files under `vendor/actus/` carry their own licenses (see the
`LICENSE.md`/`README.md` files in each subdirectory).
