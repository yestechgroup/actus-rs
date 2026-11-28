# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
will follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it
publishes its first release. Until then everything is under `Unreleased`.

## [Unreleased]

Everything since the crates were imported from `onboarding-os`.

### Added

- Engines for 18 of the 21 ACTUS dictionary v1.4 contract types: PAM, LAM,
  LAX, NAM, ANN, CLM, UMP, CSH, STK, COM, FXOUT, SWPPV, SWAPS, CAPFL, OPTNS,
  FUTUR, CEG and CEC. BCS, CDSWP and MAR have no engine yet.
- `actus-wasm`: wasm-bindgen bindings for validation, evaluation and
  applicability metadata, tested natively and in a real wasm runtime.
- `actus-web`: the ACTUS Explorer web visualizer (issue #1), with client unit
  tests run in three timezones.
- LAX is gated against the LAM testbed and the LAM oracle.
- `EngineRegistry::with_all_engines()`; `actus_engine::evaluate()` uses it.
- A byte-for-byte behaviour snapshot
  (`cargo run -p actus-conformance --example snapshot`) for proving that
  refactors change nothing.
- A guard against duplicated engine code
  (`crates/actus-engine/tests/no_duplicated_code.rs`).
- CI: a 95 % line coverage floor (`scripts/coverage.sh`), a codegen drift
  check, `cargo-deny` (licences, advisories, bans, sources) and a job that
  builds and tests on the declared MSRV.
- `rust-version = "1.88"` in the workspace manifest.
- Fuzzing.
  - `fuzz/`: cargo-fuzz targets `json_raw` and `terms_structured`, with a
    nightly CI job.
  - `crates/actus-testgen`: a structure-aware generator of extreme but
    well-formed terms for all 18 engine types, with seed contracts in
    `seeds/*.json`.
  - A stable "never panic, bounded time" property test
    (`tests/never_panics.rs`).
  - A contract-role-sign property test (`tests/properties.rs`).
- `EngineError::Arithmetic` for decimal overflow and division by zero.
- `docs/spec-interpretations.md` (SI-001 to SI-004).
- The v1.0 roadmap (`ROADMAP.md`), plus `CONTRIBUTING.md` and `SECURITY.md`.
- The 11 remaining official testbeds of the pinned `actus-tests` commit
  (STK, COM, FXOUT, SWPPV, CAPFL, OPTNS, FUTUR, CEG, UMP, LAX and the ANN
  case `ad0`; 123 cases), with every file's SHA-256 and git blob recorded
  and checked in CI (`scripts/check-vendored-testbeds.sh`). The conformance
  loader, the `report` binary and the behaviour snapshot cover all 19
  testbeds, and each new testbed has a gate that pins its exact passing
  cases.
- `EventType` gains the testbed-attested extensions `PI`, `IPFX` and `IPFL`,
  generated from `crates/actus-model/extensions/event-types.json`, with
  `EventType::DICTIONARY`, `EventType::EXTENSIONS` and `is_dictionary()`
  (SI-005).

### Changed

- Licensed under MIT (root `LICENSE`).
- Shared fixed-income helpers and the fixed-income event loop moved into
  `actus-engine::fixed_income`.
- Internal crates are declared once in `[workspace.dependencies]` with a
  version next to the path.
- tokio updated to 1.53.3 (RUSTSEC-2026-0386).
- All engine arithmetic is checked (`actus-engine/src/num.rs`), and
  `clippy::arithmetic_side_effects` is denied in `actus-engine`. Some
  `pub fn`s in `actus_engine::common` (`initial_base_amount`,
  `apply_principal_redemption`) now return `Result`.
- One contract role sign function, `common::contract_role_sign`, for every
  engine (SI-003, #38).
- `ContractReference::object` is an `Option<ReferenceObject>`: an embedded
  contract, a contract identifier (`CID`) or a market object code (`MOC`),
  chosen by `referenceType`. Use `embedded_contract()`,
  `contract_identifier()` and `market_object_code()`.
- `RiskFactorProvider::analysis_horizon()` (default none), the techspec
  `tmax`; the wasm scenario gains `analysisHorizon`. Open-ended schedules
  (STK dividends, UMP interest) without a termination or a horizon are an
  error instead of being silently cut.
- `ContractEngine::sequence_rank` lets an engine order same-day events
  differently; only CLM moves `XD` before `IP`.
- `ContractTerms::array_fixed_variable` is an `Option<Vec<ArrayFixVar>>`,
  as the dictionary's `Enum[]` type requires (#44). RUSTSEC-2026-0543 (chrono
  unmaintained) is ignored with a reason in `deny.toml`; see #32.

### Fixed

- The hand-tested types against their official testbeds (SI-006 to
  SI-009):
  - STK and COM: `quantity` defaults to 1, no notional state, an optional
    purchase, and dividends observed on `S(DVANX, DVCL, tmax)`.
  - FXOUT: no notional state, gross settlement by default (`DS = D`), and
    a net `STD` after the settlement period for `DS = S`.
  - OPTNS and FUTUR: the techspec exercise stream (`PRD`, `MD`, `XD`,
    `STD`) on the `contractStructure` underlying; the call/put uses
    `optionStrike2`.
  - UMP: scheduled interest capitalizes (`IPCI`) up to `Md`, the analysis
    horizon.
  - LAX: the maturity inferred from the principal arrays, array anchors
    without cycles, and per-segment fixed/variable rate resets with `RRF`
    (#44).
  - CAPFL: no terminal `MD` and no underlying state.
  - `nominalInterestRate2` (IPNR2) is modelled; the STK testbed's dividend
    attribute spellings are read; settlement periods accept the cycle
    notation `P5DL0`.
  - `DeliverySettlement` docs: `S` is cash, `D` is physical.
- Terms the official testbeds use and the model rejected: the calendar
  spelling `NOCALENDAR`, one-element array attributes written as bare
  scalars, `arrayFixedVariable` arrays (#44), and `contractStructure`
  references to a contract identifier or a market object.
- `actus_engine::evaluate()` used an empty registry and always returned
  `UnsupportedContractType`.
- Reachable panics on untrusted input in the engine, model and wasm crates
  now return errors.
- A carried `accruedInterest` is role-signed in PAM, LAM and LAX (#27).
- PAM no longer reports events that precede the purchase at its date (#28).
- Null elements in decimal array attributes are rejected instead of being
  dropped (#26).
- Calendar events were invisible in browsers outside UTC, plus two other view
  bugs found by the new client tests.
- Extreme terms panicked (aborting the WASM Explorer) on decimal overflow or
  division by zero in every engine (#34).
- LAM/ANN/LAX without `maturityDate` and with `nextPrincipalRedemptionPayment
  = 0` divided by zero (#35).
- UMP and CLM did not role-sign a carried `accruedInterest` (#36).
- LAX `INC` segments redeemed principal instead of increasing it. They now
  emit `PD` events (#37, SI-002).
- A CAPFL seller (`SEL`), and other roles in several engines, had the wrong
  role sign (#38).
- STK, UMP and CLM silently cut schedules after 20,000 rolls (#39).
- CEC reported `contractStatus` "active" after its maturity or settlement
  (#40).
- The WASM scenario silently dropped a credit event's `contractId`, so
  CEG/CEC never triggered through the JSON API (#41).
