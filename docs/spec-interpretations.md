# Spec interpretations

Where the ACTUS specification is ambiguous, or its sources disagree, this
log records the reading the engine implements and the evidence behind it.

Each entry gives:
- the spec section;
- the possible readings;
- the chosen reading;
- the evidence.

Evidence comes from three sources:
- **Official testbeds** (`vendor/actus/tests`). Where they decide a
  question, the entry names the case that fails when the behaviour is flipped.
- **The techspec** (`vendor/actus/techspecs/actus-techspecs.tex`, cited by
  line).
- **The actus-core reference implementation.** It was read, not run. The copy
  is actus-core 1.0.1 as mirrored in `chainaim3003/ChainAimACTUSLocal` at
  commit `007979b9eede8a1a2b29baa9c5e8a9bdcf631b15`, under `actus-core/`;
  the official `actusfrf/actus-core` repository is not publicly reachable.

Entries are numbered `SI-001`, `SI-002`, and so on. Never renumber.

---

## SI-001: `PRD`/`TD` payoffs combine the role sign with a signed `Ipac`

**Section.** PAM functions table (techspec l. 941, l. 953); LAM, NAM, ANN
and LAX inherit it ("Same as PAM"); UMP uses the PAM functions.

**Question.** The payoffs are:

- `POF_PRD = X · R(CNTRL) · (−1) · (PPRD + Ipac(t⁻) + Y · Ipnr · Nt)`
- `POF_TD = X · R(CNTRL) · (PTD + Ipac(t⁻) + Y · Ipnr · Nt)`

`Ipac` and `Nt` are already role-signed states. For `RPL` the accrued part
therefore enters with the opposite sign to the price, and a `PRD`/`TD`
payoff is not the mirror image of the `RPA` one.

**Readings.**

1. Literal: the formula as written. For `RPL`, `PRD` pays `PPRD − |Ipac|`.
2. Mirror: apply `R(CNTRL)` to the unsigned accrual, so that `RPL` exactly
   negates `RPA`.

**Chosen.** Reading 1, the literal formula.

**Evidence.**
- *Flip experiment.* Switching LAM to reading 2 fails two official cases:
  - `lam04` (`RPL`, `TD`): payoff expected −2097.836901742, reading 2 gives
    −2102.163098258;
  - `lam21` (`RPL`, `PRD`): payoff expected 5194.5205479452, reading 2 gives
    5205.4794520548.
- *Reference.* actus-core computes the same literal formula.

**Consequence.** The role-symmetry property test
(`crates/actus-testgen/tests/properties.rs`) checks every payoff except
`PRD`/`TD`. For those it checks only the states, which do mirror.

---

## SI-002: LAX `INC` segments are principal increases, reported as `PD`

**Section.** LAX schedule (techspec l. 1298, `PI` row) and LAX functions
table (l. 1402):

- `POF_PI = X · R(CNTRL) · (−1) · Nsc · Prnxt`
- `STF_PI`: `Nt⁺ = Nt⁻ + R(CNTRL) · Prnxt`, accrual and `Ipcb` as for `PR`.

**Question.** The techspec, and actus-core
(`ExoticLinearAmortizer.java` l. 128–135, `POF_PI_LAX`, `STF_PI_LAX`),
schedule a `PI` (principal increase) event for every element of an
`ARINCDEC = INC` segment. The v1.4 dictionary event vocabulary
(`actus-dictionary-event.json`), from which `actus-model` generates
`EventType`, has no `PI`. Its closest event is `PD` (principal drawing:
"drawing of principal amount").

Before this entry, the engine reported `INC` elements as `PR` and gave them
NAM-like economics: a net `Prnxt − Ipac` that *reduced* an `RPA` notional
and did not mirror under `RPL`.

**Readings.**

1. Add a non-dictionary `PI` event type.
2. Report the increase as the dictionary's `PD`.
3. Keep `PR` and fix only the amounts.

**Chosen.** Reading 2. The amounts and the direction follow the techspec
exactly; only the label maps to the dictionary vocabulary. Reading 3 would
report a principal *increase* as a *redemption*. Reading 1 would break the
rule that `EventType` covers the dictionary exactly. Switching to reading 1
later is a one-line change in `lax.rs` (`Kind::PrincipalDrawing`).

**Cap.** actus-core's `PI` functions keep the PR cap
`Prnxt − sgn · max(0, |Prnxt| − |Nt|)`, apparently copied from `PR`. The
techspec `PI` row has no cap, and the engine follows the techspec. The two
differ only when one drawing exceeds the outstanding notional.

**Evidence.** Techspec l. 1298 and l. 1402; actus-core as cited. No official
testbed covers LAX `INC` segments: the LAX gate replays the LAM cases, which
have no array terms.

---

## SI-003: one contract role sign function over the dictionary vocabulary

**Section.** "Contract Role Sign Convention" (techspec l. 533–591, table
`tbl:cntrl`).

**Question.** The techspec table lists:

| Sign | Roles |
|------|-------|
| −1 | `RPL`, `ST`, `SEL`, `PFL`, `GUA`, `UDLM` |
| +1 | `RPA`, `LG`, `BUY`, `RFL`, `COL`, `CNO`, `OBL`, `UDL`, `UDLP` |

The v1.4 dictionary role vocabulary differs: it has `RF` (receive fix) and
`PF` (pay fix), and no `LG`, `ST`, `GUA` or `OBL`.

Before this entry, the engines used four different mappings:
- the fixed-income helper signed only `RPL` as −1;
- UMP, STK, FXOUT, FUTUR and OPTNS signed `RF` (*receive* fix) as −1 and
  `PFL` as +1;
- CEC and CEG signed only `SEL` as −1.

As a result, a CAPFL sold by its creator (`SEL`) received the buyer's cash
flows.

**Readings.** For each dictionary role, the techspec sign. Two roles need a
choice:
- `RF`/`PF`: they could follow `RFL`/`PFL` (receive is a claim, pay is an
  obligation), or follow `LG`/`ST`;
- an unset role: +1, or an error.

**Chosen.** `common::contract_role_sign`, used by every engine:

| Sign | Roles |
|------|-------|
| −1 | `RPL`, `PFL`, `PF`, `SEL`, `UDLM` |
| +1 | `RPA`, `RFL`, `RF`, `BUY`, `COL`, `CNO`, `UDL`, `UDLP`, and an unset role |

SWAPS and SWPPV still accept only their leg roles `RFL`/`PFL`.

**Evidence.**
- *Techspec.* The table above.
- *Reference.* actus-core `ContractRoleConvention.roleSign` maps `RPA`, `BUY`,
  `RFL` and `RF` to +1, and `RPL`, `SEL`, `PFL` and `PF` to −1. It rejects
  the remaining roles; the engine uses the techspec signs for them instead.
- *Dictionary.* The descriptions of `RF` ("receives the fixed leg") and `PF`
  ("pays the fixed leg").
- *Snapshot.* No official testbed changes: they use `RPA`/`RPL`, `RFL`/`PFL`
  and `BUY`/`SEL` only where the old mappings already agreed.
- *Unset role.* +1 keeps every testbed that omits the role (for example the
  SWAPS legs) unchanged.

---

## SI-004: a carried `accruedInterest` is role-signed

**Section.** PAM states table, `Ipac(t₀) = IPAC` (techspec l. 853), and
`STF_IED` (l. 889). LAM, NAM, ANN, LAX, CLM and UMP inherit both.

**Question.** The techspec sets the state to the attribute `IPAC` without
applying `R(CNTRL)`. Every other monetary state, `Nt` for one, is
role-signed.

**Readings.**

1. Unsigned: the text as written.
2. Role-signed: `Ipac = R(CNTRL) · IPAC`.

**Chosen.** Reading 2, for every engine. PAM, LAM, LAX, NAM and ANN already
did this (#27); CLM and UMP did not until this entry.

**Evidence.**
- *Testbeds.* `nam04` and `lam04`, both `RPL` with a carried
  `accruedInterest`, pin the signed state (#27).
- *Reference.* actus-core signs it in the initial states of every type
  concerned, `CallMoney` (l. 162) and `UndefinedMaturityProfile` (l. 159)
  included, and its `STF_IED_PAM`, which CLM and UMP use, keeps that signed
  state.
- *Property.* Without the sign, an `RPL` contract does not mirror its `RPA`
  twin.
