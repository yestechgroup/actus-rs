# ACTUS: The algorithmic representation of financial contracts
**VERSION** v1.1-843f7a3-2020-06-08  
**AUTHOR** NILS BUNDI, ACTUS FINANCIAL RESEARCH FOUNDATION, INFO@ACTUSFRF.ORG

---

## About this document
This document provides the technical specifications of the Algorithmic Contract Types Unified Standards (ACTUS). It is developed, maintained, and released by the ACTUS Financial Research Foundation and provided by the same to the ACTUS Users Association under the terms of the open source license with which the document is published from time to time.

## Versions
This document is versioned according to the following pattern: `[major].[minor]-[revision]-[date]` where `[major]` and `[minor]` are integers marking major and minor release, `[revision]` indicates the current revision in form of the respective git commit hash (short form), and `[date]` gives the respective date of the revision. Releases are recorded in the following table.

| Date | Version | Description |
| --- | --- | --- |
| 2018-11-01 | 1.0-RC | First draft version of the technical specifications covering the ”initial” 18 contracts. |
| 2019-10-23 | 1.0 | Stable release streamlined with dictionary. Specifically, this release includes following improvements to the draft version:<br>• Fixed various naming conventions<br>• Aligned state variable names with dictionary<br>• Added ContractStructure and ”Composition”-section<br>• Added settlementCurrency attribute and updated POF accordingly<br>• Added exerciseDate and exerciseAmount terms and states for contracts with contingent payments<br>• Removed default convention and updated POF accordingly<br>• Moved Taxonomy, Event, State, Contract Role definitions to dictionary<br>• Fixed various bugs and inconsistencies |
| 2020-06-08 | 1.1 | Minor updates and fixes for alignment with dictionary-v1.2 |

## Acknowledgements
We would like to acknowledge all members of the ACTUS Users Association who contribute a lot of their time and expertise to the development, review, and testing of the ACTUS standards, in general, and this document, in particular. Without their valuable contributions the ACTUS standards would not exist in the form as they currently do.

---

## 1. Introduction
Financial contracts are legal agreements between two (or more) counterparties on the exchange of future cash flows. Such legal agreements are defined unambiguously by means of a set of contractual terms and logic. As a result, financial contracts can be described mathematically and represented digitally as machine readable algorithms.

The benefits of representing financial contracts digitally are manifold; Traditionally, transaction processing has been a field in which tremendous efficiency gains could be realized by the introduction of machines and machine readable contracts. Or, financial analytics by nature of the domain builds on the availability of computable representations of these agreements where for reasons of tractability often times analytical approximations are used. Recently, the rise of distributed ledger and blockchain technologies and the various use cases for smart contracts has opened up new possibilities for natively digital financial contracts.

In general, the exchange of cash flows between counterparties follows certain patterns. A typical cash flow exchange pattern is a bullet loan contract where principal is exchanged initially followed by cyclical interest payments and the principal is paid back (in a lump sum) at maturity of the contract. While the principal payments are fixed a variety of flavours exist for how the cyclical interest payments are determined and/or paid. As an example, interest payments may be due monthly, annually or according to arbitrary periods, they may be determined based on fixed or variable rates, different year fraction calculation methods may be used or there might be no interest due at all. Another popular pattern is that of amortizing loans for which, as opposed to bullet loans, principal may be paid out and paid back in portions of fixed or variable amounts and according to cyclical or custom schedules.

Other types of financial contracts include but are not limited to shares, forwards, options, swaps, credit enhancements, repurchase agreements, securitization, etc. By focusing on the main distinguishing features, ACTUS describes the vast majority of all financial contracts with a set of about 32 generalized cash flow exchange patterns or Contract Types (CTs), respectively. The ACTUS taxonomy provides a classification system organizing financial contracts according to their distinguishing cash flow patterns. Apart from this classification system the taxonomy also includes a description of and real-world instruments covered for each contract.

On the other hand, the legal agreements in financial contracts represent purely deterministic logic or the mechanics of finance, in other words. That is, a financial contract defines a fixed set of rules and conditions under which, given any external variables, the cash flow obligations can be determined unambiguously. For instance, in a fixed rate loan the cash flow obligations are defined explicitly. At the same time, a variable rate loan defines explicitly the rules under which the variable rate is fixed going forward such that the cash flow obligations can be derived unambiguously going forward. The same holds true for derivative contracts where the cash flow obligations arise given some underlying reference instrument. Similarly, for analytical purposes, given some assumption of the evolution of this reference instrument the cash flow obligations conditioned on this assumption can be derived unambiguously.

The properties of financial contracts described above build the foundation for a standardized, deterministic algorithmic description of the cash flow obligations arising from such agreements. Thereby, this description is technology agnostic and supports all use cases necessary for this very standard to be used throughout all finance functions from front office to back office and covering pricing, deal origination, transaction processing, as well as analytics, in general, and liquidity projections, valuation, P&L calculations and projections, and risk measurement and aggregation, in particular. Furthermore, this standard builds a formidable basis for distributed ledger-powered, natively digital financial state machines or smart contracts, in other words.

In this document, we provide the technical specification of the ACTUS standards or the mathematical description of financial contracts, in other words. We start by providing some basic notations used throughout the document followed by an introduction of the generic functions upon which financial contracts build. We continue in the following sections with an introduction of some additional foundational concepts Composition, and Risk Factor Observer and Child Contract Observer. Finally, we define the various ACTUS contracts in the last section.

---

## 2. Notations

### 2.1. Contract Attributes
Contract Attributes (attributes) represent the legal contractual terms that define the exchange of cash-flows of a financial contract. These attributes are defined and described in the ACTUS dictionary. Throughout this document attributes are referenced by their short name according to the dictionary. Further, vector-type attributes may be indexed with a subscript indicating that a specific vector-element is referenced.

* **Example 1 (Contract Attribute):** The ACTUS attribute Initial Exchange Date is referenced in short form `IED`.
* **Example 2 (Element of Vector-Type Attribute):** The ACTUS attribute Array Cycle Anchor Date of Principal Redemption is a vector-type attribute and referenced as `ARPRANX`. The i-th element of the vector is represented by $ARPRANX_i$.

### 2.2. $\emptyset$-Operator
The $\emptyset$-operator is used to indicate that a certain property is undefined or, in other words, that no value has been assigned to the respective property. In particular, for optional contract attributes it means that the attribute is not defined and for schedule times (see section 3.1) it means that the respective schedule is empty, i.e. no schedule time defined.

* **Example 3 (Undefined Attribute):** $IPANX = \emptyset$ indicates that attribute `IPANX` is undefined.
* **Example 4 (Empty Schedule):** $\tilde{t}_{IP} = \emptyset$ means the same as $\tilde{t}_{IP} = \{\}$, with $\{\}$ the empty set, and states that the IP schedule $\tilde{t}_{IP}$ does not contain a schedule time.

### 2.3. $t_0$-Time
$t_0$ represents `SD` of a contract and marks the time as per which the terms and implied state of a contract is represented. In general, from the contractual logic we are able to derive any contractual events and resulting states for any time $t > t_0$ but not for times $s < t_0$.

### 2.4. State Variables
State Variables (states) describe the state of a contract at a certain point in time $t$ during its lifetime. Examples of such states are the (outstanding) Notional Principal, the applicable Nominal Interest Rate, or the current Contract Performance. The ACTUS dictionary defines all states and provides further information on their data type, format, etc.

In general, states represent certain terms of a contract that change along the contract lifetime according to either scheduled events or unscheduled events. Therefore, states representing a contractual term carry the exact same names as their term-counterpart. States are written in their short form representation with first letter capitalized, printed in bold, and indexed with time.

* **Example 5 (State Variables):** $N_t$ refers to the state Notional Principal observed at time $t$.

### 2.5. Contract Events
A Contract Event (event) $e^k_t$ refers to any contractually scheduled or unscheduled event at a certain time $t$ and of a certain type $k$. Contract events mark specific points in time during the lifetime of a contract at which a cash flow is being exchanged (see section 2.7) or the states of the contract are being updated (see section 2.6). The dictionary lists and describes all the event types $k$ defined by the ACTUS standards.

Throughout this document event types $k$ are written in the short form as defined in the dictionary. As an event always has an associated event time $t$ and payoff $c \in \mathbb{R}$ we define two operators allowing to retrieve these quantities for any single event $e^k_t$ or set of events $\{e^k_t, e^j_s, ...\}$ as follows:

$$
\tau(x) = \begin{cases} t & \text{if } x = e^k_t \\ \{t, s, ...\} & \text{else if } x = \{e^k_t, e^j_s, ...\} \end{cases}
$$

$$
f(x) = \begin{cases} c & \text{if } x = e^k_t \\ \{c_1, c_2, ...\} & \text{else if } x = \{e^k_t, e^j_s, ...\} \end{cases}
$$
with $c_1 = f(e^k_t), c_2 = f(e^j_s), ...$

* **Example 6 (Contract Events):** The Initial Exchange Date event with event time $s$ is written as $e^{IED}_s$ with $\tau(e^{IED}_s) = s$ and $f(e^{IED}_s) = c$ where for any contract $CT$, $c = POF^{IED}_{CT}()$.

### 2.6. State Transition Functions
State Transition Functions (STF) define the transition of states from a pre-event to a post-event state when a certain event $e^k_t$ applies. Thereby, the pre-event and post-event times are indexed with $t^-$ and $t^+$, respectively. These functions are specific to a certain event and contract. STFs are written according to the following pattern `STF[event type][contract type]()` where `[event type]` and `[contract type]` refer to the respective event type and contract to which the STF belongs.

* **Example 7 (State Transition Functions):** The STF for an IP event and PAM contract is written as `STF_IP_PAM()` and maps e.g. state Accrued Interest from a pre-event state $Ipac_{t^-}$ to post-event state $Ipac_{t^+}$.

### 2.7. Payoff Functions
Payoff Functions (POF) define how the cash flow $c \in \mathbb{R}$ for a certain event $e^k_t$ is being derived from current states and from the contract terms. If necessary, the resulting cash flow can be indexed with the event time $c_t$. These functions are specific to a certain event and contract. POFs are written according to the following pattern `POF[event type][contract type]()` where `[event type]` and `[contract type]` refer to the respective event and contract to which the STF belongs.

* **Example 8 (Payoff Functions):** The POF for an IP event $e^{IP}_t$ and PAM contract is written as `POF_IP_PAM()` with $f(e^{IP}_t) = POF^{IP}_{PAM}()$.

### 2.8. Date/Time
ACTUS builds on the ISO 8601 date/time format. Hence, dates are generally expressed in the following format: `[YYYY]-[MM]-[DD]T[hh]:[mm]:[ss]`. Time zone information is currently not supported.

A special case is midnight. ISO 8601 recognizes both times `00:00:00` and `24:00:00` each referring to midnight. Yet, while `24:00:00` refers to the end of one day, `00:00:00` refers to the beginning of the following day. In ACTUS the interpretation is the same why the time period (measured in any time unit) between the two points in time will always be zero. For brevity, we use the term time for a specific date-time variable.
*A note on implementation: As many implementations of the ISO 8601 format do not support the 24:00:00 format we interpret the timestamp 23:59:59 as midnight.*

### 2.9. Event Sequence
Contract Events of different types may occur at the same time, i.e. exactly the same point in time. In this case, the sequence of evaluating their STF and POF is decisive for the resulting cash flows and state transitions. Hence, we use an event sequence indicator that can be found for each event in the event-dictionary and implies the order of executing different events at the exact same time.

### 2.10. Contract Lifetime
The lifetime of an ACTUS contract is the time period of its existence from the perspective of the analyzing user. For every point in time during its lifetime, an ACTUS contract can be analyzed in terms of current state and future cash flows.

The lifetime of a contract starts with its `SD` and ends with $\min(MD, AMD, PR^*, STD, TD, t_{max})$.
Note that $PR^*$ refers to the PR event of a maturity contract after which $N_t = 0.0$ (i.e. at which the remaining outstanding principal is redeemed). Further, $MD$, $AMD$, and $PR (N_t=0.0)$ in the definition above do only apply for maturity contracts but have to be considered infinity in all other cases. Similarly, $STD$ only applies for certain contracts and is considered infinity for all others. Finally, $t_{max}$ is a parameter that may be used to restrict the considered lifetime in an analysis. In particular, this parameter is used for contracts that do not have a natural end to their lifetime such as `STK`.

---

## 3. Utility Functions

### 3.1. Schedule
A schedule is a function $S$ mapping times $s, T$ with $s < T$ and cycle $c$ onto a sequence $\tilde{t}$ of cyclic times:

$$
S(s, c, T) = \tilde{t} = \begin{cases} 
\{\} & \text{if } s = \emptyset \land T = \emptyset \\ 
s & \text{else if } T = \emptyset \\ 
(s, T) & \text{else if } c = \emptyset \\ 
(s=t_1, ..., t_n=T) & \text{else} 
\end{cases}
$$
with $t_i < t_{i+1}, i=1,2,...$

While the schedule function can be used to create arbitrary sequences of times, it is usually used to generate sequences of cyclic events $\tilde{t}_k$ of a certain type $k$ (e.g. $k=IP$ for interest payment events) and the following build inputs to the function:
* $s = kANX$: attribute cycle anchor date of event type $k$
* $c = kCL$: event type $k$’s schedule cycle
* $T$: the schedule end date (in many cases the contract’s maturity date)

Thereby, cycles $kCL$ have format $NPS$ where:
* $N$ is an integer
* $P$ is a time period unit (`D`=Day, `W`=Week, `M`=Month, `Q`=Quarter, `H`=Half Year, `Y`=Year)
* $S$ is a stub information (`+`=long last stub, `-`=short last stub)

and with the stub defined as follows: if $t_{n-1} + c = T \lor S='-'$ then no stub correction applies, else $t_n$ is removed from the schedule.

Further, the schedule function takes a fourth, optional boolean argument $B$, i.e. $S(s, c, T, B)$ indicating whether the schedule end date $T$ belongs to the schedule or not.
* $B = T$ indicates that $T$ is part of the schedule
* $B = F$ means that $T$ is not part of the schedule

The sequence of schedule times $\tilde{t}_k$ may also be influenced by the `EOMC` and `BDC` conventions and the full function syntax becomes $S(s, c, T, EOMC, BDC)$. Due to such effects the sequence of schedule times can be non-equidistant.

### 3.2. Array Schedule
Array Schedules are defined by vector-valued inputs $\tilde{s} = (s_0, s_1, ..., s_m)$ and $\tilde{c} = (c_0, c_1, ..., c_m)$ to the array schedule function:
$$
\tilde{S}(\tilde{s}, \tilde{c}, T) = (S(s_0, c_0, s_1 - c_0), S(s_1, c_1, s_2 - c_1), ..., S(s_m, c_m, T))
$$
Hence, array schedules are a generalization for regular schedules which coincide for $m=1$. In accordance with regular schedules `EOMC` and `BDC` conventions also apply here.

### 3.3. End Of Month Shift Convention
For schedules $\tilde{t}_k$ starting at time $s$ which marks the end of a month with 30 or less days, e.g. April 30, and with a cycle $c$ being a multiple of `1M`, attribute `EOM` defines whether the schedule times are to fall on the 30th of all months (same day) or the 31st (end of month).
More specifically, `EOM` has an effect on a schedule $\tilde{t}_k$ only if:
* $s$ is the last day of a month with less than 31 days (Feb, April etc.)
* $c = NPS$ with $P \in \{M, Q, H, Y\}$

As per the dictionary `EOM` can take one of the following values:
* **EOM (EndOfMonth):** times $t_i, i=1,2,...,n-1$ are moved to the end of the respective months
* **SD (SameDay):** times $t_i, i=1,2,...,n-1$ remain unchanged except in February, where it will go to the last day if the day of month of time $s$ is higher than the number of days of February

### 3.4. Business Day Shift Convention
In general, contract events are scheduled for business days only. Therefore, the `BDC` convention defines how scheduled times $t_i, i=1,2,...,n-1$ are shifted in case they fall on a non-business day:
* **NULL:** No shift
* **SCF:** Shift/Calculate following: The event is shifted to the following non working day. Calculation of the event happens after the shift
* **SCMF:** Shift/Calculate modified following: The event is shifted to the following non working day. However, if the following day happens to fall into the next month, then take preceding non-working day. Calculation of the event happens after the shift
* **CSF:** Calculate/Shift following: Same like SCF however calculation of the event happens before the shift
* **CSMF:** Calculate/Shift modified following: Same like SCMF however calculation of the event happens before the shift
* **SCP:** Shift/Calculate preceding: The event is shifted to the last preceding non working day. Calculation of the event happens after the shift
* **SCMP:** Shift/Calculate modified preceding: The event is shifted to the last preceding non working day. However, if the preceding day happens to fall into the previous month, then take next non-working day. Calculation of the event happens after the shift
* **CSP:** Calculate/Shift preceding: Same like SCP however calculation of the event happens before the shift
* **CSMP:** Calculate/Shift modified preceding: Same like SCMP however calculation of the event happens before the shift

### 3.5. Business Day Calendar
Whether a specific day is a business day is defined by attribute `CLDR`. Such conventions generally depend on regional official holiday calendars. The Business Day Function interface allows determining for some `CLDR` whether any time $t$ is a business day or not:
$$ B: t \mapsto \{true, false\} $$
where `true` indicates that $t$ is a business day and `false` that it is a holiday.
* **Example 9:** Two standard `CLDR` implementations are `NoHoliday` (default: every calendar day is a business day) and `MondayToFriday` (all weekdays Monday-Friday are business days).

### 3.6. Year Fraction Convention
Interest income and other calculations are based on per annum interest rates. Therefore, the year-fraction function interface $Y$ is used to calculate the fraction of a year between any two times $s$ and $t$ with $t > s$ for which e.g. an (per annum) interest rate applies according to some day count convention `DCC`:
$$ Y: s, t, DCC \mapsto \mathbb{R} $$
Note, the year fraction function interface only defines the structure of year fraction functions but not an actual implementation thereof. Therefore, any `DCC` can be implemented according to the interface above supporting user-defined year fraction functions.

### 3.7. Contract Role Sign Convention
The two parties to a contract are defined through attributes `CRID` and `CPID`. The first is the party initially creating the contract and the second is the counterparty, respectively. Thereby, both `CRID`/`CPID` can take any role in the contract. The role of the `CRID` is defined through attribute `CNTRL`. The role of `CPID` is derived as the opposite side to the contract.

Contract Role Sign function $R$ maps the `CNTRL` attribute into $+1$ indicating a claim or $-1$ indicating an obligation:
$$ R: CNTRL \mapsto \{-1, +1\} $$
When multiplying with a cash flow $x$ the Contract Role Sign function thereby defines the direction of that flow: $x > 0$: $x$ flows from `CPID` to `CRID`; $x < 0$: $x$ flows from `CRID` to `CPID`.

**Table 1. Contract Role definitions.**
| Value | Meaning | R |
| --- | --- | --- |
| RPA | Real position asset | +1 |
| RPL | Real position liability | -1 |
| LG | Long position | +1 |
| ST | Short position | -1 |
| BUY | Protection buyer | +1 |
| SEL | Protection seller | -1 |
| RFL | Receive first (or fixed) leg | +1 |
| PFL | Pay first (or fixed) leg | -1 |
| COL | Collateral instrument | +1 |
| CNO | Close-out netting instrument | +1 |
| GUA | The guarantor in a Guarantee | -1 |
| OBL | The obligee in a Guarantee | +1 |
| UDL | The underlying to a composed contract | +1 |
| UDLP | The underlying to a composed contract with positive sign | +1 |
| UDLM | The underlying to a composed contract with negative sign | -1 |

### 3.8. Annuity Amount Function
In an Annuity contract (`ANN`) the annuity amount is paid regularly from the borrower to the lender. The Annuity Amount function $A$ computes the annuity amount as follows:
$$
A(s, T, n, a, r) = \frac{(n + a) \prod_{i=1}^{m-1} (1 + r Y(t_i, t_{i+1}))}{1 + \sum_{i=1}^{m-1} \prod_{j=i}^{m-1} (1 + r Y(t_j, t_{j+1}))}
$$
with $a$ the accrued interest as per time $s$, $r$ the actual interest rate, $t_i, i=1,2,...,m$ the schedule times $\inf \{t \in \tilde{t}_{PR} \mid t > s\}$, $m$ the number of times $t_i$, and $\tilde{t}_{PR}$ the PR-event schedule times of the Annuity contract.

### 3.9. Canonical Contract Payoff Function
The canonical payoff of a contract $x$ is defined as the sum of all future event payoffs evaluated under current risk factor conditions:
$$ F(x, t) = \sum_{c \in C} c $$
with $C = f(U_{ev}(x, t \mid \{O_{rf}(i, s) = O_{rf}(i, t) \forall i \land s > t\}))$.

### 3.10. Settlement Currency FX Rate
Sometimes financial contracts are settled in a different currency (i.e. the settlement currency) `CURS` than the denomination currency `CUR`. Hence, payoffs are multiplied by the respective fx-rate:
$$
X^{CURS}_{CUR}(t) = \begin{cases} 1 & \text{if } CURS = \emptyset \lor CURS = CUR \\ f(t) & \text{else} \end{cases}
$$
with $f(t) = O_{rf}(\text{concat}(CUR, '/', CURS), t)$.

---

## 4. Contract Composition
The payoff of Combined Contracts is derived from certain quantities of child contracts (also called underlying instruments or simply underlyers). In general, such child contracts can be any ACTUS contract - Basic or Combined - as well as any number of contracts. We here refer to a referenced (i.e. of lower hierarchical level) contract as a child contract and to a referencing (i.e. of higher hierarchical level) contract as a parent contract.

The ACTUS dictionary defines attribute `CTST` which captures the child contract(s) as part of the parent contract’s set of attributes. Thereby, attribute `CTST` is of type `ContractReference[]`.
We will use the following notation to query reference objects from the `CTST` attribute: $CTST^{\text{role}}_{\text{type}}$.

* **Example 10 (Underlying MarketObject-reference):** The MarketObject reference of a simple Underlying e.g. to an Option contract is referenced as $CTST^{\text{Underlying}}_{\text{MarketObjectIdentifier}}$.
* **Example 11 (FirstLeg Contract-reference):** The Contract object representing the first leg e.g. to a Swaps contract is referenced as $CTST^{\text{FirstLeg}}_{\text{Contract}}$.

---

## 5. Risk Factor Observer
The payoff of financial contracts always depends on the context in which it is evaluated and which is comprised of the following dimensions; counterparties, markets, and behavioral factors. We refer to these as the risk factors to which financial contracts are exposed to. Therefore, we consider a standardized interface $O_o(i, t, S, M)$ that allows for observing:
(1) the state of a certain risk factor $i$ at any time $t$ if $o='rf'$:
$$ O_{rf}: i, t, S, M \mapsto \mathbb{R} $$
and (2) contractual but non-scheduled events if $o='ev'$:
$$ O_{ev}: i, k, t, S, M \mapsto \{e^k_t, e^k_s, ...\} $$

* **Example 12 ('rf'-Observer):** The market-driven 3-month USD-Libor reference rate used as the variable rate in a variable rate loan contract is observed at any time $t$ through $O_{rf}(\text{MarketObjectCodeRateReset}, t)$.
* **Example 13 ('rf'-Observer):** Unscheduled (pre-) repayments of outstanding notional in a mortgage contract is observed at any time $t$ through $O_{ev}(CID, PR, t)$.

---

## 6. Child Contract Observer
In order to evaluate the derived payoff of combined contracts, we consider a standardized interface $U_o$ that allows for observing on the parent level:
(1) all future events, w.r.t. time $t$, if $o='ev'$:
$$ U_{ev}: i, t, a \mapsto \{e^k_v, e^l_w, ...\} $$
with $v, w > t$ and event types $k, l$ according to the schedule of the child contract,
(2) a certain state variable $x$ if $o='sv'$:
$$ U_{sv}: i, t, x, a \mapsto \mathbb{R} $$
or (3) a particular contract attribute $x$ of the child contract if $o='ca'$:
$$ U_{ca}: i, x \mapsto y $$

* **Example 14 ('ev'-Observer):** The future events, w.r.t. time $t$, of the first leg of a `SWAPS` contract with `CNTRL=PFL` can be evaluated as $U_{ev}(CTST^{\text{FirstLeg}}_{\text{Contract}}, t \mid \{CNTRL=RPL\})$.
* **Example 15 ('sv'-Observer):** The current state, w.r.t. time $t$, of state variable $N_t$ of the first leg of a `SWAPS` contract with `CNTRL=RFL` can be evaluated as $U_{sv}(CTST^{\text{FirstLeg}}_{\text{Contract}}, t, N_t \mid \{CNTRL=RPA\})$.
* **Example 16 ('ca'-Observer):** The contract attribute `MOC` of the child contract `Child` of an `OPTNS` contract can be evaluated as $U_{ca}(CTST^{\text{Underlying}}_{\text{Contract}}, MOC)$.

---

## 7. Contract Types

### 7.1. PAM: Principal At Maturity

#### PAM: Contract Schedule
| Event | Schedule | Comments |
| --- | --- | --- |
| AD | $\tilde{t}_{AD} = (t_0, t_1, ..., t_n)$ | With $t_i, i=1,2,...$ a custom input |
| IED | $t_{IED} = IED$ | |
| MD | $t_{MD} = Md_{t_0}$ | |
| PP | $\tilde{t}_{PP} = \begin{cases} \emptyset & \text{if } PPEF = 'N' \\ (\tilde{u}, \tilde{v}) & \text{else} \end{cases}$ <br> where $\tilde{u} = S(s, OPCL, T_{MD})$, $\tilde{v} = O_{ev}(CID, PP, t)$ | with $s = \begin{cases} \emptyset & \text{if } OPANX = \emptyset \land OPCL = \emptyset \\ IED + OPCL & \text{else if } OPANX = \emptyset \\ OPANX & \text{else} \end{cases}$ |
| PY | $\tilde{t}_{PY} = \begin{cases} \emptyset & \text{if } PYTP = 'O' \\ \tilde{t}_{PP} & \text{else} \end{cases}$ | |
| FP | $\tilde{t}_{FP} = \begin{cases} \emptyset & \text{if } FER = \emptyset \lor FER = 0 \\ S(s, FECL, T_{MD}) & \text{else} \end{cases}$ | with $s = \begin{cases} \emptyset & \text{if } FEANX = \emptyset \land FECL = \emptyset \\ IED + FECL & \text{else if } FEANX = \emptyset \\ FEANX & \text{else} \end{cases}$ |
| PRD | $t_{PRD} = PRD$ | |
| TD | $t_{TD} = TD$ | |
| IP | $\tilde{t}_{IP} = \begin{cases} \emptyset & \text{if } IPNR = \emptyset \\ S(s, IPCL, T_{MD}) & \text{else} \end{cases}$ | with $s = \begin{cases} \emptyset & \text{if } IPANX = \emptyset \land IPCL = \emptyset \\ IPCED & \text{else if } IPCED \neq \emptyset \\ IED + IPCL & \text{else if } IPANX = \emptyset \\ IPANX & \text{else} \end{cases}$ |
| IPCI | $\tilde{t}_{IPCI} = \begin{cases} \emptyset & \text{if } IPCED = \emptyset \\ S(s, IPCL, IPCED) & \text{else} \end{cases}$ | with $s = \begin{cases} \emptyset & \text{if } IPANX = \emptyset \land IPCL = \emptyset \\ IED + IPCL & \text{else if } IPANX = \emptyset \\ IPANX & \text{else} \end{cases}$ |
| RR | $\tilde{t}_{RR} = \begin{cases} \tilde{t} \setminus \{t_{RRY}\} & \text{if } RRANX = \emptyset \land RRCL = \emptyset \\ \tilde{t} & \text{else if } RRNXT \neq \emptyset \end{cases}$ <br> where $\tilde{t} = S(s, RRCL, T_{MD})$ | with $s = \begin{cases} IED + RRCL & \text{if } RRANX = \emptyset \\ RRANX & \text{else} \end{cases}$ <br> $t_{RRY} = \inf \{t \in \tilde{t} \mid t > SD\}$ |
| RRF | $t_{RRF} = \begin{cases} \emptyset & \text{if } RRANX = \emptyset \land RRCL = \emptyset \\ \inf \{t \in \tilde{t} \mid t > SD\} & \text{else} \end{cases}$ <br> where $\tilde{t} = S(s, RRCL, T_{MD})$ | with $s = \begin{cases} IED + RRCL & \text{if } RRANX = \emptyset \\ RRANX & \text{else} \end{cases}$ |
| SC | $\tilde{t}_{SC} = \begin{cases} \emptyset & \text{if } SCEF = '000' \\ S(s, SCCL, T_{MD}) & \text{else} \end{cases}$ | with $s = \begin{cases} \emptyset & \text{if } SCANX = \emptyset \land SCCL = \emptyset \\ IED + SCCL & \text{else if } SCANX = \emptyset \\ SCANX & \text{else} \end{cases}$ |
| CE | $\tilde{t}_{CE} = \{t \mid Prf_{t^+} \neq Prf_{t^-}\}$ | |

#### PAM: State Variables Initialization
| State | Initialization per $t_0$ | Comments |
| --- | --- | --- |
| Md | $Md_{t_0} = MD$ | |
| Nt | $N_{t_0} = \begin{cases} 0.0 & \text{if } IED > t_0 \\ R(CNTRL) \times NT & \text{else} \end{cases}$ | |
| Ipnr | $Ipnr_{t_0} = \begin{cases} 0.0 & \text{if } IED > t_0 \\ IPNR & \text{else} \end{cases}$ | |
| Ipac | $Ipac_{t_0} = \begin{cases} 0.0 & \text{if } IPNR = \emptyset \\ IPAC & \text{else if } IPAC \neq \emptyset \\ Y(t^-, t_0) \times N_{t_0} \times Ipnr_{t_0} & \text{else} \end{cases}$ | with $t^- = \sup \{t \in \tilde{t}_{IP} \mid t < t_0\}$ |
| Feac | $Feac_{t_0} = \begin{cases} 0.0 & \text{if } FER = \emptyset \\ FEAC & \text{else if } FEAC \neq \emptyset \\ Y(t_{FP^-}, t_0) \times N_{t_0} \times FER & \text{else if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t_0)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ | with $t_{FP^-} = \sup \{t \in \tilde{t}_{FP} \mid t < t_0\}$, $t_{FP^+} = \inf \{t \in \tilde{t}_{FP} \mid t > t_0\}$ |
| Nsc | $Nsc_{t_0} = \begin{cases} SCIXSD & \text{if } SCEF = '[x]N[x]' \\ 1.0 & \text{else} \end{cases}$ | |
| Isc | $Isc_{t_0} = \begin{cases} SCIXSD & \text{if } SCEF = 'I[x][x]' \\ 1.0 & \text{else} \end{cases}$ | |
| Prf | $Prf_{t_0} = PRF$ | |
| Sd | $Sd_{t_0} = t_0$ | |

#### PAM: State Transition Functions and Payoff Functions
| Event | Payoff Function | State Transition Function |
| --- | --- | --- |
| AD | $0.0$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Sd_{t^+} = t$ |
| IED | $X^{CURS}_{CUR}(t) R(CNTRL) (-1) (NT + PD_{IED})$ | $N_{t^+} = R(CNTRL) NT$ <br> $Ipnr_{t^+} = \begin{cases} 0.0 & \text{if } IPNR = \emptyset \\ IPNR & \text{else} \end{cases}$ <br> $Ipac_{t^+} = \begin{cases} IPAC & \text{if } IPAC \neq \emptyset \\ y N_{t^+} + Ipnr_{t^+} & \text{if } IPANX \neq \emptyset \land IPANX < t \\ 0.0 & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ with $y = Y(IPANX, t)$ |
| MD | $X^{CURS}_{CUR}(t) (Nsc_{t^-} - N_{t^-} + Isc_{t^-} Ipac_{t^-} + Feac_{t^-})$ | $N_{t^+} = 0.0$, $Ipac_{t^+} = 0.0$, $Feac_{t^+} = 0.0$, $Sd_{t^+} = t$ |
| PP | $X^{CURS}_{CUR}(t) f(O_{ev}(CID, PP, t))$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $N_{t^+} = N_{t^-} - f(O_{ev}(CID, PP, t))$ <br> $Sd_{t^+} = t$ |
| PY | $X^{CURS}_{CUR}(t) R(CNTRL) PYRT$ if $PYTP='A'$ <br> $c PYRT$ if $PYTP='N'$ <br> $c \max(0, Ipnr_{t^-} - O_{rf}(RRMO, t))$ if $PYTP='I'$ <br> with $c = X^{CURS}_{CUR}(t) R(CNTRL) Y(Sd_{t^-}, t) N_{t^-}$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ |
| FP | $R(CNTRL) c$ if $FEB='A'$ <br> $c Y(Sd_{t^-}, t) N_{t^-} + Feac_{t^-}$ if $FEB='N'$ <br> with $c = X^{CURS}_{CUR}(t) FER$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = 0.0$, $Sd_{t^+} = t$ |
| PRD | $X^{CURS}_{CUR}(t) R(CNTRL) (-1) (PPRD + Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-})$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ |
| TD | $X^{CURS}_{CUR}(t) R(CNTRL) (PTD + Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-})$ | $N_{t^+} = 0.0$, $Ipac_{t^+} = 0.0$, $Feac_{t^+} = 0.0$, $Ipnr_{t^+} = 0.0$, $Sd_{t^+} = t$ |
| IP | $X^{CURS}_{CUR}(t) Isc_{t^-} (Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-})$ | $Ipac_{t^+} = 0.0$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ |
| IPCI | $0.0$ | $N_{t^+} = N_{t^-} + Ipac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} Ipnr_{t^-}$ <br> $Ipac_{t^+} = 0.0$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ |
| RR | $0.0$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Ipnr_{t^+} = \min(\max(Ipnr_{t^-} + \Delta r, RRLF), RRLC)$ <br> $Sd_{t^+} = t$ with $\Delta r = \min(\max(O_{rf}(RRMO, t) RRMLT + RRSP - Ipnr_{t^-}, RRPF), RRPC)$ |
| RRF | $0.0$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Ipnr_{t^+} = RRNXT$, $Sd_{t^+} = t$ |
| SC | $0.0$ | $Ipac_{t^+} = Ipac_{t^-} + Y(Sd_{t^-}, t) Ipnr_{t^-} N_{t^-}$ <br> $Feac_{t^+} = \begin{cases} Feac_{t^-} + Y(Sd_{t^-}, t) N_{t^-} FER & \text{if } FEB = 'N' \\ \frac{Y(t_{FP^-}, t)}{Y(t_{FP^-}, t_{FP^+})} R(CNTRL)FER & \text{else} \end{cases}$ <br> $Nsc_{t^+} = \begin{cases} Nsc_{t^-} & \text{if } SCEF=[x]0[x] \\ \frac{O_{rf}(SCMO, t) - SCIED}{SCIED} & \text{else} \end{cases}$ <br> $Isc_{t^+} = \begin{cases} Isc_{t^-} & \text{if } SCEF=0[x][x] \\ \frac{O_{rf}(SCMO, t) - SCIED}{SCIED} & \text{else} \end{cases}$ <br> $Sd_{t^+} = t$ |
| CE | $0.0$ | `STF_AD_PAM()` |

*(Note: Due to length constraints, subsequent contract types follow the exact same structural logic, referencing PAM/LAM where applicable. Full mathematical definitions for LAM, LAX, NAM, ANN, CLM, UMP, CSH, STK, COM, FXOUT, SWPPV, SWAPS, CAPFL, OPTNS, FUTUR, CEG, and CEC are preserved in the standard ACTUS dictionary format.)*

### 7.2. LAM: Linear Amortizer
* **Schedule:** `AD`, `IED`, `MD`, `PP`, `PY`, `FP`, `PRD`, `TD`, `IP`, `IPCI`, `RR`, `RRF`, `SC`, `CE` are Same as PAM.
* **PR Schedule:** $t_{PR} = S(s, PRCL, T_{MD}, F)$ with $s = \begin{cases} \emptyset & \text{if } PRANX = \emptyset \land PRCL = \emptyset \\ IED + PRCL & \text{else if } PRANX = \emptyset \\ PRANX & \text{else} \end{cases}$
* **IPCB Schedule:** $\tilde{t}_{IPCB} = \begin{cases} \emptyset & \text{if } IPCB \neq 'NTL' \\ S(s, IPCBCL, T_{MD}) & \text{else} \end{cases}$
* **State Variables:** `Md`, `Nt`, `Ipnr`, `Ipac`, `Feac`, `Nsc`, `Isc`, `Prf`, `Sd` follow PAM logic with adjustments for `Prnxt` and `Ipcb`.
* **STF/POF:** Follows PAM logic, adjusting principal redemption tracking via `Prnxt` and interest calculation base via `Ipcb`.

### 7.3. LAX: Exotic Linear Amortizer
* Extends LAM with Array Schedules for Principal Redemption (`PR`, `PI`), Rate Resets (`RR`, `RRF`), and Interest Payments (`IP`).
* Uses `ARPRANX`, `ARPRCL`, `ARINCDEC` for principal schedules.
* Uses `ARRRANX`, `ARRRCL`, `ARFIXVAR` for rate schedules.

### 7.4. NAM: Negative Amortizer
* Similar to LAM but allows principal to increase if interest payments are less than accrued interest.
* **IP Schedule:** Adjusted to align with principal redemption schedules.

### 7.5. ANN: Annuity
* Follows NAM schedule logic.
* **STF/POF:** Uses the Annuity Amount Function $A(t, Md_{t^+}, N_{t^+}, Ipac_{t^+}, Ipnr_{t^+})$ to compute `Prnxt` at rate reset events.

### 7.6. CLM: Call Money
* Undefined maturity profile driven by unscheduled events.
* **IP Schedule:** $t_{IP} = Md_{t_0}$

### 7.7. UMP: Undefined Maturity Profile
* Driven entirely by unscheduled `PR` and `PI` events observed via $O_{ev}(CID, i, t_0)$.

### 7.8. CSH: Cash
* Simplest contract. Only `AD` event. State $N_t = R(CNTRL) NT$.

### 7.9. STK: Stock
* Represents equity. Payoffs driven by market observations $O_{rf}$.
* **DV Schedule:** Dividend events based on `DVANX`, `DVCL`.

### 7.10. COM: Commodity
* Similar to STK, represents physical or synthetic commodity holdings.

### 7.11. FXOUT: Foreign Exchange Outright
* Exchange of two currencies at `STD`.
* **Payoff:** $X^{CURS}_{CUR}(t) R(CNTRL) (NT - O_{rf}(i, Md_t) NT_2)$ where $i = \text{concat}(CUR_2, '/', CUR)$.

### 7.12. SWPPV: Plain Vanilla Interest Rate Swap
* Combines fixed and floating legs.
* **State Variables:** Tracks `Ipac1` (fixed) and `Ipac2` (floating).
* **IP Payoff:** Net settlement of fixed vs floating accrued interest.

### 7.13. SWAPS: Swap
* Generic swap combining any two child contracts (`FirstLeg`, `SecondLeg`).
* Merges congruent events of child contracts into aggregate events $z_m^\tau$.

### 7.14. CAPFL: Cap-Floor
* Optionality on interest rates.
* Evaluates underlying child contract with and without caps/floors (`RRLC`, `RRLF`) and takes the absolute difference.

### 7.15. OPTNS: Option
* **XD Schedule:** Exercise date derived from underlying contract events.
* **MD Payoff:** Intrinsic value calculation based on `OPTP` ('C' or 'P') and underlying market object $S_t$.

### 7.16. FUTUR: Future
* Similar to OPTNS but tracks forward price `PFUT`.
* **MD Payoff:** $S_t - PFUT$.

### 7.17. CEG: Credit Enhancement Guarantee
* Guarantees covered contracts.
* **Nt Initialization:** Aggregates exposure from covered contracts using `CECVR`.
* **XD Event:** Triggered by credit events on covered contracts.

### 7.18. CEC: Credit Enhancement Collateral
* Collateral backing a guarantee.
* **Nt Initialization:** Minimum of collateral market value and guaranteed exposure.
