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
| `tests/actus-tests-*.json` | All 19 official conformance testbeds of the pinned commit, 277 cases (table below) |
| `techspecs/actus-techspecs.tex` | Algorithmic specification (state transition functions \(\vec{v}_{t+1}\), payoff functions \(R_t\), schedule generation, day count, cycles) |
| `techspecs/VERSION.md` | Techspec update log |

## Vendored testbeds

Every file is byte-identical to `tests/<file>` of `actusfrf/actus-tests` at
the pinned commit `f7a8064872b69db1f0beabac771c99dc3ce0c397`, which is also
upstream `HEAD` as of 2026-11-28. The git blob column is `git hash-object`
of the file and equals the blob in the upstream tree
(`git ls-tree f7a8064 tests/`); the SHA-256 column is repeated in
`[package.metadata.actus.tests_sha256]` and checked in CI by
`scripts/check-vendored-testbeds.sh`. All carry the CC-BY-SA-4.0 licence of
`tests/LICENSE.md`.

| File | Type | Cases | SHA-256 | Git blob | Vendored |
| --- | --- | --- | --- | --- | --- |
| `actus-tests-pam.json` | PAM | 25 | `cf08dc73b63a6916a6667fd8119542b6b45c6f4d3f9409a3af528a4832255c94` | `318201025a1e6ed2ff3c61e91b63f15f2efa6fe9` | 2026-09-20 (e68814e) |
| `actus-tests-lam.json` | LAM | 31 | `fb39c0f62aa0d71462c48b363c381c503774f78825d0cfdc18fab7613f2f1ba2` | `9d557f9adc13f00f812078001d9b1ffa98352aa2` | 2026-09-20 (e68814e) |
| `actus-tests-nam.json` | NAM | 22 | `bfc39c7a344b1243ce15accb9800837b3d87a050ffa435395595e4bf559e4a9b` | `5344260727106ab511d08a1aef4ea75449e18096` | 2026-09-20 (e68814e) |
| `actus-tests-ann.json` | ANN | 31 | `24fe7359740e6f5a05f763e6305361d6af98224214caa466a29b4bc5f243bee5` | `af87d556cc920e773d1ac570ae8b1ae06e8dfc92` | 2026-09-20 (e68814e) |
| `actus-tests-clm.json` | CLM | 15 | `260946fabce675391677b62875578ae1338934820f64c191b64ffbcd67740ccb` | `da52eeb5f59d65cc028fbd21c04b59ed0f472129` | 2026-09-20 (e68814e) |
| `actus-tests-csh.json` | CSH | 4 | `b8ecc6420d818450d22c2f79e65a4279028d06565f8dd52f1011de6c5ff865ba` | `359af0247ab5898e84cff5552351ac68ca646daf` | 2026-09-20 (e68814e) |
| `actus-tests-swaps.json` | SWAPS | 11 | `7033d684f93503d24eb01ccc842a74c19b2d4946a697f409176844074178a125` | `0de099eb81c81f9b7af44691ef2bdc226ac9f18c` | 2026-09-20 (e68814e) |
| `actus-tests-cec.json` | CEC | 15 | `3ee4789b5644962db8426f73363a432cb293e915f5682b9bfa930c99f8f90559` | `55e3baabb513157c8a9dfb41ed1257c1e2520a39` | 2026-09-20 (e68814e) |
| `actus-tests-stk.json` | STK | 10 | `f6baee4b887056bc34f818e38e0abb378d31f7b745f21b9d579df952dc0d95bf` | `551706e7a732bd2ff71ccb75fd31a2ef42e7a255` | 2026-11-28 |
| `actus-tests-com.json` | COM | 4 | `c0f1f7fc7f7d1149348e72a7771300d9c7a6c413fce64926baf04b17a75302b0` | `f277d6bfd8a96a0b008fc7489d46efb73d80e28c` | 2026-11-28 |
| `actus-tests-fxout.json` | FXOUT | 12 | `c7f6a002d8b2bd3d75302c74c146dd65850c27907d87198a3ea62e45872fe2ae` | `ae9f1a70e49132868790b4fa169a5a5d87c64a4b` | 2026-11-28 |
| `actus-tests-swppv.json` | SWPPV | 14 | `a01ed0af87d0a1eb403205de8eae5c69ab2516ee72005d920ad9bbf2878e43a0` | `4b9392debca963ac40c987417037c22d017aec36` | 2026-11-28 |
| `actus-tests-capfl.json` | CAPFL | 4 | `4f2b3953413e7bb91fb8e4c658feae09d963dcb4d26d4fe693b8fcafc60bd420` | `189dd585e4da312fc6cc44c98b118bc8b012d70f` | 2026-11-28 |
| `actus-tests-optns.json` | OPTNS | 23 | `d97712ce8c3a7f067b06a8c49bfe95cbbed12bd78b27cb5c9ea398958e0cd42e` | `613b3a0353a49c942004bafd82afe4050f47551d` | 2026-11-28 |
| `actus-tests-futur.json` | FUTUR | 14 | `d30423222dbb9fef3eb906f6ac76d7dbc008b0edced28d2d26a31b21c681aae1` | `bee027d41f18af87144836cc811ce19a09ccbac7` | 2026-11-28 |
| `actus-tests-ceg.json` | CEG | 14 | `660e74c0ccad100fbe4adc1045ffe6ed9feac034e9a8bce16ce12a0e5e84db59` | `f4413fec7c0a64eeb38639d40538f02f5c56d779` | 2026-11-28 |
| `actus-tests-ump.json` | UMP | 9 | `6b98f2a663d27099579e6b2a0365a161a82ec84f1fb51fd31660b4afd26f152f` | `723116afcdec8fd650f9a19e0a2d861d77577a1d` | 2026-11-28 |
| `actus-tests-lax.json` | LAX | 18 | `74772315713bbebe980814faecc09f0b1f39ae67a5cfebd65fd353621367859a` | `f9342c0b3e11724da4af48d20744e580db001b98` | 2026-11-28 |
| `actus-tests-ad0.json` | ANN | 1 | `5fccaa0ec5323bcb0ef4df1407199b30622182a82e61b0bbf25793ebade18aab` | `d4da70d4a7250c0b3dae2d41cbe266f7f84f3dd6` | 2026-11-28 |

`actus-tests-ad0.json` holds one ANN case (identifier `ann01`, with an `AD`
monitoring event); the conformance harness reports it as `ANN (ad0)`.

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
5. The FXOUT testbed spells the calendar `NC` as **`NOCALENDAR`** (11 of 12
   cases), the upper-cased dictionary identifier `noCalendar`. `actus-model`
   accepts it as an alias of `NC` (`enums.rs`, `Calendar`).
6. The LAX testbed writes **one-element arrays as bare scalars**:
   `"arrayCycleOfPrincipalRedemption": "P1YL1"`,
   `"arrayNextPrincipalRedemptionPayment": 20` (lax01–lax11, and the
   interest-payment arrays of lax16–lax18, next to true arrays in the same
   bag). The dictionary types are arrays (`Cycle[]`, `Real[]`, ...);
   `actus-model` reads a bare scalar as a one-element array
   (`serde_helpers.rs`, "Singleton arrays") and serializes the array form.
7. The LAX testbed spells `arrayFixedVariable` elements **`FIX`/`VAR`**, the
   dictionary description's spelling, not the acronyms `F`/`V`; both parse
   (`enums.rs`, `ArrayFixVar`).
8. The testbeds use **three event types the v1.4 dictionary lacks**: `PI`
   (LAX principal increase) and `IPFX`/`IPFL` (SWPPV fixed and floating leg
   interest). The techspec defines all three. `actus-model` adds them through
   its generator from `crates/actus-model/extensions/event-types.json`
   (`docs/spec-interpretations.md`, SI-005).

## Purpose

1. **Conformance** — `cargo run -p actus-conformance` evaluates the vendored
   testbeds and reports expected vs actual events; the same run gates CI as an
   integration test.
2. **Normative reference** — `crates/actus-model` generates its vocabulary
   (contract types, event types, attributes, applicability) from the dictionary,
   and `crates/actus-engine` implements the algorithms from the techspec.
   When upstream changes, regenerate and re-diff against these pinned copies.
