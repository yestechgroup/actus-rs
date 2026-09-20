# actus-rs

ACTUS in Rust: the [ACTUS](https://www.actusfrf.org/) Data Standard vocabulary,
contract terms, and the Algorithmic Standard execution engine, plus a
differential conformance harness against the official actusfrf testbeds.

| Crate | Purpose |
|-------|---------|
| `actus-model` | Contract terms vocabulary generated from the ACTUS dictionary; vendored testbed vectors |
| `actus-engine` | Schedule generation, state variables, event sequencing, day counts; contract types PAM, ANN, LAM, NAM, CEC, CLM, CSH + swaps |
| `actus-conformance` | Deterministic seeded corpus, oracle, comparator; differential tests vs the actusfrf testbeds |

Design properties: **pure** (no I/O, no clock, no randomness — results derive
from inputs alone), `rust_decimal::Decimal` money, `chrono` timestamps,
hermetic tests (the conformance corpus is seeded in-memory and pinned by hash;
the actusfrf dictionary/techspecs/tests are vendored under `vendor/actus/`).

## Status: dormant, conformance-tested

The engine is complete for the v1 family (see `crates/actus-conformance/src/corpus.rs`
for the covered/excluded matrix) and CI runs the differential conformance suite
on every push.

**Doctrine:** no new contract types, conventions, or features without a named
consumer. Integration into a product toolchain is **deferred until a consumer
names it** — a payment-schedule screen or API, or a second contract family.
When that trigger fires: start with a thin mapper in the product codebase
(first-template terms → `actus-engine::evaluate` for the needed types only);
promote to a toolchain seam (e.g. a `rex_ir` consumer per yestechgroup/rexlang#10)
only when a second consumer appears.

History note: these crates originated in [onboarding-os](https://github.com/yestechgroup/onboarding-os)
(see its ADR 0001); that repo's `financial-domain` and `onboarding-extensions`
consume `actus-model`/`actus-engine` by git rev pin.

## Build & test

```bash
cargo test --workspace        # includes the differential conformance suite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

Apache-2.0. The vendored ACTUS dictionary, techspecs, and testbed files under
`vendor/actus/` carry their own licenses (see the `LICENSE.md`/`README.md`
files in each subdirectory).
