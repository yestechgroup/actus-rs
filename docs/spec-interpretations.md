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

---

## SI-005: testbed-attested event types outside the v1.4 dictionary

**Section.** Dictionary event vocabulary (`actus-dictionary-event.json`);
LAX schedule and functions (techspec l. 1298, l. 1402, the `PI` rows);
SWPPV schedule and functions (l. 2130, l. 2135, l. 2211, l. 2215, the
`IP^(fix)` and `IP^(var)` rows).

**Question.** `actus-model` generates `EventType` from the dictionary, and
a test requires it to cover the dictionary exactly. Three event types that
the techspec defines, and that the official testbeds expect, are not in the
dictionary:

| Token | Meaning | Techspec | Testbed |
|-------|---------|----------|---------|
| `PI` | LAX principal increase (`ARINCDEC = INC`) | l. 1298, l. 1402 | `actus-tests-lax.json`, 18 events (lax16–lax18) |
| `IPFX` | SWPPV fixed-leg interest, `DS = 'D'` | `IP^(fix)`, l. 2130, l. 2211 | `actus-tests-swppv.json`, 45 events (10 cases) |
| `IPFL` | SWPPV floating-leg interest, `DS = 'D'` | `IP^(var)`, l. 2135, l. 2215 | `actus-tests-swppv.json`, 45 events (10 cases) |

**Readings.**

1. Keep `EventType` to the dictionary; the testbeds that use these tokens
   cannot even be loaded.
2. Map each token to its nearest dictionary event (`PI` → `PD`, as SI-002
   did; `IPFX`/`IPFL` → `IP`).
3. Add the three tokens to `EventType`, kept apart from the dictionary
   vocabulary.

**Chosen.** Reading 3, through the generator rather than by hand:
- `crates/actus-model/extensions/event-types.json` lists each extension
  with its identifier, sequence, techspec lines and testbed files. It is
  crate data, not vendored data.
- The generator appends them to `EventType` with a "Testbed extension"
  doc comment. `EventType::DICTIONARY` is exactly the dictionary,
  `EventType::EXTENSIONS` exactly the extensions, and `ALL` both;
  `is_dictionary()` tells them apart. The generator rejects an extension
  that collides with a dictionary acronym or identifier.
- `tests/dictionary.rs` checks that `EXTENSIONS` equals the set of event
  types the vendored testbeds use minus the dictionary's. The vocabulary
  widens only by testbed evidence, and a testbed bump that drops or adds
  one fails the test.
- Sequences: `PI` takes `PR`'s 3 (it is the `INC` counterpart of `PR` in
  the LAX schedule; the testbed orders same-day `PI, IP`). `IPFX` and `IPFL`
  take `IP`'s 8; the testbed orders same-day `IPFX, IPFL, RR`, so an
  engine emits `IPFX` first and the stable sort keeps it there.

`EventType` stays exhaustive (the maintainers chose not to add
`#[non_exhaustive]` in this batch).

**Evidence.** The techspec rows above, and the testbed counts. Reading 1
makes 3 of 18 LAX cases and 10 of 14 SWPPV cases unloadable. Reading 2
would report a principal *increase* as a drawing and merge legs the
techspec keeps apart when `DS = 'D'`. Which engines emit the extensions is
decided per type (SI-002 for LAX `PI`; SWPPV in its own entry).

---

## SI-006: STK and COM follow the techspec; the testbeds settle price, quantity and dividends

**Section.** "STK: Stock" and "COM: Commodity" (techspec l. 1955–2030):
schedules, states and functions. Dictionary attributes `quantity` (QT),
`nextDividendPaymentAmount` (DVNP), `cycleOfDividend` (DVCL).

**Question.** The engines were written from the paper (`vendor/actus/docs/paper.md`
§7.9–7.10) with a share-count model: a position notional `sgn · QT · PPRD`,
a required `quantity` and purchase, and `DVNP` paid on every dividend
date. The techspec and the newly vendored STK and COM testbeds disagree
with that in five places.

**Readings and evidence.**

| Point | Paper-derived engine | Techspec | Testbed | Chosen |
|-------|----------------------|----------|---------|--------|
| Notional state | `sgn · QT · PPRD` | no `Nt` (states: `Prf`, `Sd`) | `notionalPrincipal` 0 everywhere | none |
| `quantity` | required | absent from every payoff | STK: never set, payoffs `= PPRD`; COM: `PRD = 2 × 700` | `QT` multiplies, default 1 (dictionary default) |
| Purchase | required | `PRD` "Same as PAM", only when set | com02, stk08–10 have none | optional |
| Dividend series | `DVANX`, `DVCL` to `TD`, or the anchor alone | `S(s, DVCL, tmax)` | stk06–08 run quarterly to the horizon `to` | to `tmax`, the analysis horizon (or `TD`) |
| Dividend amount | `DVNP` every period | `DV^(fix)` pays `DVNP` at `DVANX`; `DV` pays `O(concat(CID, "-DV"), t)` | observed series named by `marketObjectCodeOfDividends` | observed; code from `marketObjectCodeOfDividends`, else `CID-DV` |

Details of the choices:

- **Quantity.** The techspec's COM payoffs reuse `POF_PRD_STK = R · (−1) ·
  PPRD`, without `QT`, yet the COM testbed multiplies by it (com01:
  `QT = 2`, `PPRD = 700`, `PRD` pays 1400; com02: `TD` pays `2 × 900`).
  "`QT` multiplies, with the dictionary default 1" fits both testbeds; the
  STK cases, which never set `quantity`, pay the price itself. `QT` also
  multiplies the dividends (per-share dividends of a position), which no
  testbed can decide, since none sets both.
- **Horizon.** `tmax` is the analysis horizon, carried by the evaluation
  environment (`RiskFactorProvider::analysis_horizon`; the testbed `to`;
  the wasm scenario's `analysisHorizon`). The series includes `tmax` when
  it falls on the cycle (stk06 pays on its horizon 2018-01-01) and ends
  before `TD`. A dividend cycle with neither bound is open-ended and an
  error, where the engine used to pay the anchor alone, a silently wrong
  answer.
- **Dividend series code.** *Flip experiment:* observing the techspec's
  `concat(CID, "-DV")` instead of `marketObjectCodeOfDividends` fails
  stk06–stk10 (STK drops from 10/10 to 5/10). The testbed names the series
  through `marketObjectCodeOfDividends`, an attribute the dictionary lacks
  (carried like `fixingDays`; known quirk 9). `CID-DV` stays the fallback.
- **`DVNP`.** The dictionary describes `DVNP` as "used as dividend payment
  for each future dividend payment date" when `DVCL` is set. The techspec
  pays it once, as `DV^(fix)` at `DVANX`, and starts the observed series
  at `DVANX + DVCL`. No testbed case sets `DVNP`. The techspec ranks above
  the dictionary (`CONTRIBUTING.md`), so the engine follows it.
- **Business days.** Dividend dates follow `BDC`/`CLDR` (stk09: 2017-01-01
  is a Sunday, and `SCF` pays on 2017-01-02).
- **Not changed.** `IED` (paper-only for STK and COM; the techspec schedule
  has none, and no testbed sets it) and the sale price precedence
  (market observation, then `PTD`, then `MVO`; com02's `PTD` wins over its
  `MVO`) stay as they were.

**Result.** STK 10/10, COM 4/4.

---

## SI-007: FXOUT settles gross by default, reported as `MD`; fxout10's `TD` sign is a testbed defect

**Section.** "FXOUT: Foreign Exchange Outright" (techspec l. 2044–2105):
`STD` (`DS = 'S'`), `STD^(1)` and `STD^(2)` (`DS = 'D'`); `TD` paying
`POF_TD_STK = X · R(CNTRL) · PTD`. Dictionary `deliverySettlement`,
default `D`.

**Questions and readings.**

1. *Which settlement applies when `DS` is unset?* The engine settled net
   at the observed rate, which errors without an fx observation. The
   dictionary default is `D`, so the techspec's gross `STD^(1)`/`STD^(2)`
   apply.
2. *What are the gross legs called?* The techspec names them `STD^(1)`
   and `STD^(2)`; the testbed reports two `MD` events, one per currency.
3. *When does the net `STD` settle, and at which rate?* The testbed:
   at `MD + settlementPeriod`, at the rate observed at `MD`.
4. *Is the `TD` payoff role-signed?* `POF_TD_STK = R(CNTRL) · PTD` is.

**Chosen.** Gross legs by default, reported as `MD` in their own
currencies (`R · NT` in `CUR`, `R · (−1) · NT2` in `CUR2`); the net `STD`
at `MD + STP`, valued at `MD`; `TD` role-signed.

**Evidence.**
- *Default.* fxout01, fxout02 and fxout07 set no `DS` and expect the two
  gross legs; without an fx observation the old net reading could not
  even evaluate them.
- *Naming (flip experiment).* Reporting the gross legs as `STD`, as the
  techspec names them, fails fxout01, fxout02 and fxout07.
- *Net settlement.* fxout03, 04, 05, 09, 11 and 12 settle at
  `MD + STP` (`P1DL0`, `P5DL0`) with the rate dated `MD`.
- *`TD` sign: a testbed defect.* fxout10 is `RPL` with `PTD = 100` and
  expects `TD = +100`; the techspec gives `R(CNTRL) · PTD = −100`. The same
  reference implementation signs the same function `POF_TD_STK` for STK
  (stk05: `RPL`, `PTD = 3000`, `TD = −3000`) and COM (com04: `RPL`,
  `TD = −2700`), and fxout10's own `PRD` is role-signed
  (`R · (−1) · PPRD = −50`). An unsigned `TD` would pass fxout10 and change
  no other FXOUT case: no FXOUT case contradicts it, but the techspec does,
  and so do the reference's own STK and COM outputs. The engine keeps the
  signed payoff; the FXOUT gate pins fxout10 as failing (11/12), and the
  vendored file stays untouched.

---

## SI-008: OPTNS and FUTUR settle an exercise amount; the testbeds sign the premium and keep zero exercises

**Section.** "OPTNS: Option" and "FUTUR: Future" (techspec l. 2395–2540):
schedules `PRD`, `TD`, `MD`, `XD`, `STD`; states `Xd`, `Xa`; `STF_MD`,
`STF_XD`, `POF_STD = X · R(CNTRL) · Xa`.

**Question.** The engines were written from the paper: OPTNS paid
`R · NT · max(±(S − OPS1), 0)` at `MD`, required `notionalPrincipal`, read
the underlying from the contract's own `marketObjectCode` and never used
`optionStrike2`. FUTUR paid `R · NT · (S − PFUT)` at `MD` and rejected
`DS = S` as physical delivery (the model's `DeliverySettlement` docs had
the two tokens swapped). None of that matches the techspec, and no
testbed case evaluated.

**Chosen.** The techspec event stream, implemented once in
`crates/actus-engine/src/exercise.rs` (FUTUR is "Same as OPTNS"):
`PRD`, `MD` (zero), `XD` (zero; fixes `Xa`), and `STD` at
`Xd + settlementPeriod`, shifted per `BDC`/`CLDR`, paying `R(CNTRL) · Xa`.
`Xa` is `max(S − OPS1, 0)` for a call, `max(OPS1 − S, 0)` for a put,
`max(S − OPS1, 0) + max(OPS2 − S, 0)` for a call/put, and `S − PFUT` for a
future. The underlying `S` is the market object of the `UDL` reference in
`contractStructure`. There is no notional state. A contract already
exercised (`XD ≤ t0`) settles its `exerciseAmount`.

Two testbed deviations from the techspec text:

1. **The premium is role-signed.** The techspec writes
   `POF_PRD_OPTNS = X · (−1) · PPRD`, without `R(CNTRL)`; the testbed pays
   the seller `+PPRD` (option04: `SEL`, `PRD` +10). *Flip experiment:* the
   unsigned techspec payoff fails option04–07 and option23, every `SEL`
   case. `TD` is signed the same way (`R · PTD`); no case decides it.
2. **A zero exercise is still reported.** By `STF_MD`, `Xd` stays
   undefined when `x = 0`, so no `XD` or `STD` would follow; the testbed
   reports `XD` and a zero `STD` (option01: `Xa = 0`). *Flip experiment:*
   dropping them for `Xa = 0` fails option01, option12 and option16–21.

**Sequence.** The same-day order is the dictionary's `MD, XD, STD`
(option01–23). The CLM-only rank that puts `XD` before `IP` is now CLM's
own (`event::call_money_sequence_rank`).

**Out of scope.** American and Bermudan exercise (`OPXT` `A`/`B`), which
the techspec drives by observed `XD` events, remain an error unless the
contract carries its exercise date; no testbed case uses them.

---

## SI-009: the `ad0` testbed expects an `AD` event its inputs do not define (testbed defect)

**Section.** PAM schedule, `AD` row (techspec l. 778):
`t^AD = (t_0, t_1, ..., t_n)` "with `t_i`, i = 1, 2, ..., a custom
input". ANN inherits it ("Same as PAM").

**Question.** `actus-tests-ad0.json` holds one ANN case (`ann01`) whose
expected results include an `AD` event at 2013-03-31T23:59:59. The case
has no input that could produce it: the terms carry no analysis-date
attribute (the dictionary has none), and `eventsObserved` and
`dataObserved` are empty.

**Chosen.** A testbed defect: the case lacks its custom `AD` input. The
engine is not changed to invent the date, and the vendored file stays
untouched; the `ad0` gate pins the case as failing (0/1).

**Evidence.**
- With the `AD` row removed from the expected results, the engine
  reproduces the other 25 events exactly (scratch comparison through the
  conformance comparator).
- The expected `AD` state (`accruedInterest` 28.4857..., the accrual
  `4192.4640650764 × 0.08 × 31/365` from 2013-03-01) shows the reference
  evaluated an analysis date at 2013-04-01, which the case never passes in.
- Supplying that date as an observed `AD` event does not help: the
  fixed-income engines do not yet replay custom `AD` dates at all (only CSH
  does). That gap is filed separately; it is a feature for user-supplied
  analysis dates, not something this case can decide.
