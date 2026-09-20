# Vendored: ACTUS Data Standard + Algorithmic Standard (dictionary v1.4)

Normative inputs for the ACTUS execution engine (`crates/actus-model`,
`crates/actus-engine`, `crates/actus-conformance`). These directories are the
**external conformance suite** — they are not modified by this repo, and nothing
under `schemas/` is generated from them.

- **Source repos** (ACTUS Financial Research Foundation, CC-BY-SA-4.0):
  - https://github.com/actusfrf/actus-dictionary — pinned at commit `356f7663f26091105cc4fef4ae3496942dcf0ebf` (dictionary v1.4, 2023-12-08)
  - https://github.com/actusfrf/actus-tests — pinned at commit `f7a8064872b69db1f0beabac771c99dc3ce0c397`
  - https://actusfrf.github.io/actus-techspecs via https://github.com/actusfrf/actus-techspecs — pinned at commit `94ef09e4992f79d573f84f41d8480f557365870e`
- The pinned revisions are also recorded in `[package.metadata.actus]` in
  `crates/actus-conformance/Cargo.toml`. Upstream warns the standards, reference
  implementation and testbeds are subject to change — re-diff before bumping.

## Contents

| Path | Purpose |
| --- | --- |
| `dictionary/actus-dictionary-{terms,states,event,taxonomy,applicability,versions,contract-reference}.json` | Machine-readable data dictionary: contract attributes, state variables, event types, contract taxonomy, attribute applicability per contract type |
| `tests/actus-tests-{pam,lam,nam,ann,clm,csh,swaps,cec}.json` | Official conformance testbeds (PAM 25 cases, LAM 31, NAM 22, ANN 31, CLM 15, CSH 4, SWAPS 11, CEC 15) |
| `techspecs/actus-techspecs.tex` | Algorithmic specification (state transition functions \(\vec{v}_{t+1}\), payoff functions \(R_t\), schedule generation, day count, cycles) |
| `techspecs/VERSION.md` | Techspec update log |

## Reference snapshots (NOT vendored)

The upstream Java reference is the `org.actus:actus-core:1.1.0` Maven artifact, gated behind an
auth token issued by actusfrf.org; the dependency embedding pattern is visible in the Apache-2.0
`actus-service` repo (`implementation("org.actus:actus-core:1.1.0")`). Committing its CC-BY-SA-4.0
reference outputs as pinned snapshots requires a license check first; until then CI runs the
in-Rust oracle only (`crates/actus-conformance/src/oracle.rs`, `tests/differential.rs`).

## Known quirks (encoding of the upstream artifacts)

1. `dictionary/actus-dictionary-terms.json` is **not valid strict JSON** — it
   contains typographic curly quotes inside description strings. Consumers must
   pre-normalise (`“`/`”` → `"`) before parsing (see the generator in
   `crates/actus-model`).
2. Testbed `eventDate` values use **minute precision without seconds**
   (`2013-01-01T00:00`), unlike `terms.statusDate` (`2013-01-01T00:00:00`).
   Both must parse (see the conformance loader).
3. Testbed numeric fields (payoffs, states, risk-factor observations) arrive as
   strings or JSON numbers — both are f64-printed by the upstream Java reference.
   The conformance comparator therefore uses an absolute 0.01 (one cent) /
   relative 1e-9 tolerance rather than exact equality.
4. `applicability` entries for unreleased contract types contain working notes
   (`"Tbd ..."`) rather than applicability values; only released types
   (PAM/LAM/NAM/ANN for this engine) have usable matrices.

## Purpose

1. **Conformance** — `cargo run -p actus-conformance` evaluates the vendored
   testbeds and reports expected vs actual events; the same run gates CI as an
   integration test.
2. **Normative reference** — `crates/actus-model` generates its vocabulary
   (contract types, event types, attributes, applicability) from the dictionary,
   and `crates/actus-engine` implements the algorithms from the techspec.
   When upstream changes, regenerate and re-diff against these pinned copies.
