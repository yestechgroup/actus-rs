# actus-rs roadmap to v1.0

v1.0 means two things at once:

1. **Trustworthy.** The engine is formally verified where we can verify it,
   differentially tested where we can't, and every ACTUS spec formula traces
   to code and to a test.
2. **Showable.** The repo, docs and the ACTUS Explorer web UI look like a
   finished product. Someone landing on the README should understand ACTUS,
   trust the numbers and be able to play with every contract type in a
   browser within a minute.

The GitHub milestones match the sections below one to one. Run
`scripts/roadmap/create-milestones.sh` to create them and assign the existing
issues (it needs `gh` with write access).

---

## Where we are (review, 2026-11-27)

### Strengths

- **Clean pure core.** `actus-engine` does no I/O, reads no clock and uses no
  randomness. It has no `unsafe`, uses `rust_decimal` for money and has a
  single dispatch seam (`ContractEngine` / `EngineRegistry`). Clippy
  (`-D warnings`) and rustfmt are clean.
- **Real conformance, not a smoke test.** All 8 official actusfrf testbeds
  pass at the event level: **154/154 contracts and 3418/3418 events**
  (`cargo run -p actus-conformance --bin report`).
- **An independent oracle.** `actus-conformance::oracle` reimplements
  PAM/LAM/NAM/ANN from the spec text on `i128` fixed point and shares no
  code with the engine. It is compared against the engine on a pinned,
  hash-locked corpus of 2048 seeded cases with a derived error bound. This is
  the most valuable verification asset in the repo.
- **Generated vocabulary.** Terms, events, contract types and applicability
  are generated from the vendored ACTUS dictionary v1.4, and tests check that
  the generated code covers the dictionary exactly.
- **Engines exist for 18 of the 21 dictionary contract types.**
- **Tests:** about 356, all green in about 1 s. Workspace line coverage is
  **88.2 %** (cargo-llvm-cov).

### Gaps and risks

| Area | Finding |
|------|---------|
| Verification breadth | Only 8 of 18 engines have official testbeds, and only the 4 fixed-income types have the oracle. The other 10 (STK, COM, FXOUT, SWPPV, CAPFL, OPTNS, FUTUR, CEG, UMP, LAX) rely on 5–9 hand-computed unit tests each. |
| Known defects | Issues #2–#8 are spec-behaviour gaps in the loan family: PP/PY events, contract performance, scaling re-fixing, post-t0 status date crash, LAM over-redemption. |
| Corpus exclusions | Scaling (`SC`), fees, prepayment/penalty, purchase/termination, progressed contracts (`IED < SD`), the `Aa`/`30E360ISDA`/`28E336` day counts, and non-`NT` interest calculation base are all excluded from differential testing (`corpus.rs` docs). |
| Missing types | BCS, CDSWP and MAR are in the dictionary as "Implemented" but have no engine. |
| No formal methods | There are no proofs yet. Schedule and day count code is small, total and bounded, which makes it a natural target for Kani. |
| Panics in library code | There are 24 `unwrap`/`expect`/`panic!` sites outside test modules in engine, model and wasm, 11 of them in `schedule.rs` and 3 in `daycount.rs`. They look like invariants, but none are documented or proven, and untrusted JSON reaches them through WASM. These are good first targets for Kani. |
| Public API footgun | `actus_engine::evaluate()` uses an **empty** registry and always returns `UnsupportedContractType`. Its doc comment still says "in this wave". The real registry is copy-pasted in `actus-conformance` and `actus-wasm`. |
| Coverage holes | `lax.rs` 72 %, `serde_helpers.rs` 47 %, `cycle.rs` 79 %, `actus-wasm/lib.rs` 33 %, `actus-web/lib.rs` 35 %. Neither the generator nor the web UI has tests (no e2e, no TS tests). CI does not measure coverage. |
| Repo hygiene | There is no `LICENSE` file at the root even though the README says Apache-2.0. There is no CHANGELOG, CONTRIBUTING, SECURITY, MSRV or `cargo-deny`, nothing is published to crates.io, and there are no `#![deny(missing_docs)]` / `#![forbid(unsafe_code)]` crate attributes. |
| Doc drift | `crates/actus-web/README.md` says 8 engine-backed types, but 18 are wired. The root README "dormant / no new features" doctrine contradicts a v1.0 push and needs rewriting. |
| Web UI | It works and is hydrated SSR, but it shows every contract type as a fixed-income timeline. There are no per-family visualizations (swap legs, option payoff, state over time), no shareable URLs, no hosted demo and no browser tests. It depends on `microsoft-webui 0.0.29`, a pre-1.0 framework, which is a long-term risk for a flagship demo. |

---

## Milestones

### v0.2 — Foundations

Tracking: [#9](https://github.com/yestechgroup/actus-rs/issues/9)

Fix the cheap and embarrassing things first.

- Add a root `LICENSE` (Apache-2.0), plus `CHANGELOG.md`, `CONTRIBUTING.md`
  and `SECURITY.md`.
- Add `actus_engine::default_registry()` with all 18 engines and make
  `evaluate()` use it. Remove the duplicate registries in conformance and wasm.
- Add `#![forbid(unsafe_code)]` and `#![warn(missing_docs)]` to every crate.
- CI: coverage with cargo-llvm-cov and an enforced floor (start at 88 %),
  `cargo-deny` (licenses and advisories), a declared and tested MSRV, and a
  drift check that regenerates `actus-model/src/generated` and fails on any
  diff.
- Rewrite the README status/doctrine section for the v1.0 direction and fix
  the doc drift in the web README.
- Cover the holes: `lax.rs`, `serde_helpers.rs`, `cycle.rs` and the wasm
  `lib.rs` entry points.

### v0.3 — Spec completeness

Tracking: [#10](https://github.com/yestechgroup/actus-rs/issues/10)

Close every known gap against the ACTUS techspec.

- Fix issues #2–#8.
- Implement the remaining dictionary types BCS, CDSWP and MAR, or record a
  decision to defer them with reasons.
- Close the exclusions: the `SC` scaling event, fees (`FP`), prepayment and
  penalty (`PP`/`PY`), purchase/termination (`PRD`/`TD`), progressed
  contracts, the `Aa`/`30E360ISDA`/`28E336` day counts and the `NTIED`/`NTL`
  interest calculation bases.
- Error policy: library code must not panic on any input that type-checks.
  Convert the reachable sites among the 24 `unwrap`/`expect` calls to
  `EngineError`. Keep only documented invariants, each with an
  `// INVARIANT:` comment (they are proven in v0.5).
- Write a spec-reading log (`docs/spec-interpretations.md`) that collects
  every ambiguity resolution currently scattered across module docs.

### v0.4 — Verification depth

Tracking: [#11](https://github.com/yestechgroup/actus-rs/issues/11)

Every contract type gets a second, independent opinion.

- Extend the oracle (or add sibling oracles) to UMP, CLM, CSH, STK, COM,
  FXOUT, SWPPV, SWAPS, CAPFL, OPTNS, FUTUR, CEG and CEC, and extend the corpus
  generator to match.
- Cross-implementation differential against the reference Java `actus-core`:
  a one-off recorder produces JSON vectors, which are vendored so the tests
  stay hermetic. This covers every type without an official testbed.
- Property tests per contract type, for example: notional conservation,
  non-negative balances where the spec requires them, payoff sign by role,
  monotone schedules, idempotent re-evaluation, and parity relationships
  (cap − floor = swap leg, call − put = forward).
- Fuzzing with `cargo-fuzz` on terms JSON → `validate` → `evaluate`: it must
  never panic, never loop forever and always terminate with a bounded number
  of events. Run it in CI nightly.
- Mutation testing with `cargo-mutants` on `daycount`, `schedule`, `state`
  and the fixed-income engines. Target: no surviving mutants in the core.
- Raise the coverage floor to 95 % for `actus-engine` and `actus-model`.

### v0.5 — Formal verification

Tracking: [#12](https://github.com/yestechgroup/actus-rs/issues/12)

Machine-checked guarantees for the parts that every contract depends on.

- **Kani** bounded proofs (run in CI):
  - Day counts: totality, no overflow, `yf(a,a) = 0`, sign and additivity
    where the convention guarantees it, and the 30E/360 family's day-clamping
    rules.
  - Schedule roll: termination, strict monotonicity, EOM/SD semantics, stub
    handling, and that business-day shifting never reorders events.
  - Event ordering: `sort_events` is a total order consistent with the
    dictionary sequence numbers.
  - No panics in `schedule`, `daycount` and `state` for all inputs within
    the date range.
- Checked-arithmetic audit: every `Decimal` operation in payoff/state code is
  either `checked_*` → `EngineError::Overflow`, or comes with a proven bound.
- **Spec traceability matrix**, generated and published: every techspec
  POF/STF function (for example `POF_IP_PAM`, `STF_PR_LAM`) maps to the
  implementing function and the tests and proofs that cover it. CI fails if a
  spec function has no implementation or no test.
- Investigate Verus or Creusot for functional-correctness proofs of the
  annuity formula and PAM accrual, and pick one as a v1.x stretch goal.

### v0.6 — Stable API and packaging

Tracking: [#13](https://github.com/yestechgroup/actus-rs/issues/13)

Make it consumable by people who aren't us.

- Public API review: `#[non_exhaustive]` on enums and errors, builder
  ergonomics, a consistent naming scheme, and a decision on the
  `RiskFactorProvider` shape.
- Stable, versioned JSON schemas for terms input and event output, published
  and validated in tests.
- `cargo-semver-checks` in CI. Publish `actus-model` and `actus-engine` to
  crates.io as 0.x pre-releases, and publish `actus-wasm` to npm.
- Rustdoc for docs.rs: a runnable example on every public item and a crate
  overview that explains ACTUS for newcomers.
- Criterion benchmarks (events/s per type, large portfolio) with regression
  tracking.

### v0.8 — ACTUS Explorer

Tracking: [#14](https://github.com/yestechgroup/actus-rs/issues/14)

A web UI that makes ACTUS legible.

- **Per-family visualizations**:
  - Loans: cash-flow waterfall, plus a chart of notional and accrued interest
    over time.
  - Swaps: two legs and the net.
  - Options and futures: payoff diagram and exercise event.
  - Credit enhancement: the covered contract and the guarantee/collateral
    overlay.
  - Stock and commodity: dividends and the termination price.
- A **state-variable inspector**: click an event and see the state before and
  after, the POF/STF formula that applied and a link to the spec section.
  This doubles as the user-facing face of the traceability matrix.
- **Gallery of presets** taken from the official testbeds, each badged
  "matches reference ✓".
- Shareable permalinks (terms encoded in the URL), JSON import/export and
  side-by-side contract comparison.
- Responsive layout, dark mode, keyboard navigation, WCAG AA contrast.
- **Static deploy to GitHub Pages**. The WASM engine already runs
  client-side, so the hosted demo needs no server. Decide whether to keep
  `microsoft-webui` 0.0.x or move to a static SPA build.
- Playwright e2e tests, visual regression snapshots and TS unit tests for
  `view.ts`.
- Close #1.

### v1.0 — Launch

Tracking: [#15](https://github.com/yestechgroup/actus-rs/issues/15)

- README: hero screenshot or GIF of the Explorer, live-demo link, badges
  (CI, coverage, conformance 100 %, Kani proofs, crates.io, docs.rs), a
  30-second quickstart and a "why trust this" section that links the
  conformance report and the traceability matrix.
- Docs site (mdBook) published with the demo: an ACTUS primer, one page per
  contract type with a live embedded example, verification methodology,
  spec interpretations and the API guide.
- A published conformance and verification report, regenerated on every
  release.
- Release automation (release-plz or cargo-release), signed tags, crates.io
  1.0 and npm 1.0.
- Project identity: logo, social preview image, `CITATION.cff`, plus an
  announcement post or JOSS-style paper and outreach to the ACTUS community.
- **Exit criteria:**
  - 100 % of official testbeds pass.
  - Every engine is differentially tested against an independent
    implementation.
  - Kani proofs are green in CI.
  - Zero open correctness issues.
  - The public API is semver-checked.
  - The demo loads in under 2 s with no console errors.
