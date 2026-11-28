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
All 19 official actusfrf testbeds are vendored (277 cases). How thoroughly
each type is checked is recorded in
[`crates/actus-conformance/verification.toml`](crates/actus-conformance/verification.toml),
which CI holds against the testbeds (`tests/verification.rs`):

- **A**: official testbed, plus an independent oracle and a 2048-case
  differential corpus;
- **B**: official testbed, passing except documented testbed defects;
- **C**: no complete official testbed yet.

<!-- verification-tiers:start -->
| Type | Tier | Official testbed | Oracle | Notes |
|---|---|---|---|---|
| PAM | A | `pam` 25/25 | yes |  |
| LAM | A | `lam` 31/31 | yes |  |
| NAM | A | `nam` 22/22 | yes |  |
| ANN | A | `ann` 31/31, `ad0` 0/1 | yes | testbed defect: ad0/ann01 (SI-009) |
| LAX | B | `lax` 18/18 | — | also replayed against the LAM testbed and the LAM oracle (tests/lax_parity.rs) |
| CLM | B | `clm` 15/15 | — |  |
| CSH | B | `csh` 4/4 | — |  |
| SWAPS | B | `swaps` 11/11 | — |  |
| CEC | B | `cec` 15/15 | — |  |
| STK | B | `stk` 10/10 | — |  |
| COM | B | `com` 4/4 | — |  |
| FXOUT | B | `fxout` 11/12 | — | testbed defect: fxout/fxout10 (SI-007) |
| OPTNS | B | `optns` 23/23 | — |  |
| FUTUR | B | `futur` 14/14 | — |  |
| UMP | B | `ump` 9/9 | — |  |
| CAPFL | B | `capfl` 4/4 | — |  |
| CEG | C | `ceg` 0/14 | — | #48 |
| SWPPV | C | `swppv` 0/14 | — | #49 |
| BCS | none | — | — | no engine |
| CDSWP | none | — | — | no engine |
| MAR | none | — | — | no engine |
<!-- verification-tiers:end -->

The testbed defects and every other spec reading are recorded with their
evidence in [`docs/spec-interpretations.md`](docs/spec-interpretations.md).
`cargo run -p actus-conformance --bin report` prints the per-case report.

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

See `crates/actus-web/README.md` for details. The engines for the 18 v1
contract types follow the techspec (`vendor/actus/techspecs`), gated on the
official testbeds as the verification table above records.

## License

MIT, see [`LICENSE`](LICENSE). The vendored ACTUS dictionary, techspecs, and
testbed files under `vendor/actus/` carry their own licenses (see the
`LICENSE.md`/`README.md` files in each subdirectory).
