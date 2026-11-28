//! SWPPV: Plain Vanilla Interest Rate Swap (techspec "SWPPV", l. 2107–2235).
//!
//! The techspec's standalone contract: one notional, a fixed rate `IPNR`
//! and a floating rate `Ipnr` that starts at `IPNR2` and resets per the
//! `RR` schedule. There is no principal exchange and no contract structure;
//! swaps composed of two leg contracts are SWAPS.
//!
//! **States.** `Nt = sgn x NT` from `IED`; `Ipnr` (0 before `IED`, then
//! `IPNR2`, then `RRMLT x obs(RRMO, t) + RRSP` at each `RR`); the leg
//! accruals `Ipac1` (fixed, `Y x Nt x IPNR`) and `Ipac2` (floating,
//! `Y x Nt x Ipnr`); the net accrual `Ipac = Ipac1 - Ipac2`. The reported
//! states are `notionalPrincipal = Nt`, `nominalInterestRate = Ipnr` and
//! `accruedInterest = Ipac1` (SI-013).
//!
//! **Schedule.** `IED` (payoff 0), `PRD`, `TD`, `IP` dates from `IPANX`
//! (else `IED + IPCL`, else only `MD`) per `IPCL` to `MD`, `RR` dates from
//! `RRANX` (else `IED + RRCL`) per `RRCL`, and `MD` (payoff 0). Per
//! `deliverySettlement` (SI-010), each `IP` date pays:
//!
//! - `DS = 'S'`: one net `IP` of `Ipac1 - Ipac2 + Y x (IPNR - Ipnr) x Nt`;
//! - `DS = 'D'` (the dictionary default): `IPFX` of `Ipac1 + Y x IPNR x Nt`,
//!   then `IPFL` of `-(Ipac2 + Y x Ipnr x Nt)`.
//!
//! `Nt` carries the role sign, so the payoffs apply no second `sgn`
//! (SI-014), and `IPFL` negates the whole floating accrual (SI-015).
//! `PRD` pays `-sgn x PPRD` and `TD` pays `sgn x PTD` (`POF_PRD_STK`,
//! `POF_TD_STK`). Events before `PRD` change the state but are not
//! reported; nothing is reported after `TD`.
//!
//! **Roles.** `RF` (receive fix, `+1`) and `PF` (pay fix, `-1`), with the
//! leg roles `RFL`/`PFL` accepted as synonyms (SI-003).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractRole, ContractTerms, ContractType, DayCountConvention,
    DeliverySettlement, EndOfMonthConvention, EventType,
};

use crate::common::{contract_role_sign, stub_series};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::fixed_income::{self, ScheduledEvent};
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the SWPPV contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct SwppvEngine;

impl ContractEngine for SwppvEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Swppv
    }

    /// Evaluates the standalone swap: the schedule in (calculation time,
    /// sequence) order, through the fixed-income event loop.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let ctx = Context::new(terms)?;
        let slots = ctx.schedule(terms)?;
        let (start, mut floating) = if ctx.ied < ctx.t0 {
            ctx.progressed(terms, &slots)?
        } else {
            (ContractState::initial(terms), Decimal::ZERO)
        };
        let pre_ied = start.clone();
        fixed_income::evaluate_schedule(
            ctx.ied,
            ctx.t0,
            slots,
            terms,
            || Ok(start),
            || Ok(pre_ied),
            |state, slot| ctx.apply(state, &mut floating, slot, terms, risk),
        )
    }
}

/// The kinds of SWPPV schedule elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    Purchase,
    InterestPayment,
    InterestPaymentFixed,
    InterestPaymentFloating,
    RateReset,
    Termination,
    Maturity,
}

impl Kind {
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::Purchase => EventType::Purchase,
            Kind::InterestPayment => EventType::InterestPayment,
            Kind::InterestPaymentFixed => EventType::InterestPaymentFixedLeg,
            Kind::InterestPaymentFloating => EventType::InterestPaymentFloatingLeg,
            Kind::RateReset => EventType::RateResetVariable,
            Kind::Termination => EventType::Termination,
            Kind::Maturity => EventType::Maturity,
        }
    }
}

/// One schedule element.
#[derive(Debug, Clone, Copy)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

impl ScheduledEvent for Slot {
    fn event_type(&self) -> EventType {
        self.kind.event_type()
    }

    fn sequence(&self) -> u8 {
        self.kind.event_type().priority()
    }

    fn emit(&self) -> NaiveDateTime {
        self.emit
    }
}

/// Resolved attributes and conventions of one SWPPV evaluation.
struct Context {
    t0: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
    sgn: Decimal,
    notional: Decimal,
    fixed_rate: Decimal,
    initial_floating_rate: Decimal,
    net: bool,
}

impl Context {
    fn new(terms: &ContractTerms) -> Result<Context, EngineError> {
        if terms.contract_structure.is_some() {
            return Err(EngineError::InvalidTransition(
                "SWPPV is a standalone contract without a contract structure; \
                 a swap composed of leg contracts is SWAPS"
                    .to_string(),
            ));
        }
        match terms.contract_role {
            None
            | Some(ContractRole::Rf | ContractRole::Pf | ContractRole::Rfl | ContractRole::Pfl) => {
            }
            Some(other) => {
                return Err(EngineError::InvalidTransition(format!(
                    "unsupported SWPPV contract role {}",
                    other.as_token()
                )))
            }
        }
        Ok(Context {
            t0: ContractState::initial(terms).status_date,
            ied: terms
                .initial_exchange_date
                .map(normalize_timestamp)
                .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?,
            maturity: terms
                .maturity_date
                .map(normalize_timestamp)
                .ok_or(EngineError::MissingAttribute("maturityDate"))?,
            dcc: terms
                .day_count_convention
                .ok_or(EngineError::MissingAttribute("dayCountConvention"))?,
            eomc: terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd),
            bdc: terms
                .business_day_convention
                .unwrap_or(BusinessDayConvention::Nos),
            cal: terms.calendar.unwrap_or(Calendar::Nc),
            sgn: contract_role_sign(terms.contract_role),
            notional: terms
                .notional_principal
                .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?,
            fixed_rate: terms
                .nominal_interest_rate
                .ok_or(EngineError::MissingAttribute("nominalInterestRate"))?,
            initial_floating_rate: terms
                .nominal_interest_rate2
                .ok_or(EngineError::MissingAttribute("nominalInterestRate2"))?,
            net: terms.delivery_settlement == Some(DeliverySettlement::S),
        })
    }

    /// The schedule in (calculation time, sequence) order; `IPFX` precedes
    /// `IPFL` on the same date (the sort is stable).
    fn schedule(&self, terms: &ContractTerms) -> Result<Vec<Slot>, EngineError> {
        let at = |kind, time| Slot {
            kind,
            calc: time,
            emit: time,
        };
        let mut slots = vec![
            at(Kind::InitialExchange, self.ied),
            at(Kind::Maturity, self.maturity),
        ];
        if let Some(prd) = terms.purchase_date {
            slots.push(at(Kind::Purchase, prd));
        }
        if let Some(td) = terms.termination_date {
            slots.push(at(Kind::Termination, td));
        }
        for (calc, emit) in self.interest_dates(terms)? {
            let kinds: &[Kind] = if self.net {
                &[Kind::InterestPayment]
            } else {
                &[Kind::InterestPaymentFixed, Kind::InterestPaymentFloating]
            };
            for &kind in kinds {
                slots.push(Slot { kind, calc, emit });
            }
        }
        for (calc, emit) in self.rate_reset_dates(terms)? {
            slots.push(Slot {
                kind: Kind::RateReset,
                calc,
                emit,
            });
        }
        slots.sort_by_key(|slot| (slot.calc, slot.kind.event_type().priority()));
        Ok(slots)
    }

    /// The `IP` dates: `S(IPANX or IED + IPCL, IPCL, MD)`, or `MD` alone
    /// without a cycle.
    fn interest_dates(
        &self,
        terms: &ContractTerms,
    ) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>, EngineError> {
        let Some(cycle) = terms.cycle_of_interest_payment.as_ref() else {
            return Ok(vec![(self.maturity, self.maturity)]);
        };
        let anchor = match terms.cycle_anchor_date_of_interest_payment {
            Some(anchor) => normalize_timestamp(anchor),
            None => cycle_step(self.ied, 1, cycle, self.eomc)?,
        };
        stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
    }

    /// The `RR` dates: `S(RRANX or IED + RRCL, RRCL, MD)` without the
    /// maturity element; a reset anchor without a cycle is a single reset
    /// (swppv01).
    fn rate_reset_dates(
        &self,
        terms: &ContractTerms,
    ) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>, EngineError> {
        let anchor = terms
            .cycle_anchor_date_of_rate_reset
            .map(normalize_timestamp);
        let mut series = match (anchor, terms.cycle_of_rate_reset.as_ref()) {
            (Some(anchor), None) => vec![(anchor, anchor)],
            (anchor, Some(cycle)) => {
                let anchor = match anchor {
                    Some(anchor) => anchor,
                    None => cycle_step(self.ied, 1, cycle, self.eomc)?,
                };
                stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)?
            }
            (None, None) => Vec::new(),
        };
        series.retain(|(calc, _)| *calc < self.maturity);
        Ok(series)
    }

    /// The techspec `t0` states of a swap running at the status date
    /// (`IED < t0`): `Nt = sgn x NT`, `Ipnr = IPNR2`, and the leg accruals
    /// from the last `IP` date before `t0` (else `IED`). A carried
    /// `accruedInterest` is the net accrual `Ipac`; it is reported as the
    /// fixed-leg accrual with no floating accrual.
    fn progressed(
        &self,
        terms: &ContractTerms,
        slots: &[Slot],
    ) -> Result<(ContractState, Decimal), EngineError> {
        let mut state = ContractState::initial(terms);
        state.notional_principal = (ck(self.sgn) * self.notional).get()?;
        state.nominal_interest_rate = self.initial_floating_rate;
        let last_payment = slots
            .iter()
            .filter(|slot| {
                matches!(
                    slot.kind,
                    Kind::InterestPayment | Kind::InterestPaymentFixed
                ) && slot.calc < self.t0
            })
            .map(|slot| slot.calc)
            .max()
            .unwrap_or(self.ied);
        let yf = day_count_fraction(last_payment, self.t0, self.dcc)?;
        let floating = match terms.accrued_interest {
            Some(accrued) => {
                state.accrued_interest = (ck(self.sgn) * accrued).get()?;
                Decimal::ZERO
            }
            None => {
                state.accrued_interest =
                    (ck(yf) * state.notional_principal * self.fixed_rate).get()?;
                (ck(yf) * state.notional_principal * self.initial_floating_rate).get()?
            }
        };
        state.status_date = self.t0;
        Ok((state, floating))
    }

    /// Applies the state transition and payoff function of one element
    /// (techspec SWPPV functions table) and returns the payoff.
    ///
    /// `state.accrued_interest` holds `Ipac1`, `floating` holds `Ipac2`.
    fn apply(
        &self,
        state: &mut ContractState,
        floating: &mut Decimal,
        slot: Slot,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Decimal, EngineError> {
        let yf = day_count_fraction(state.status_date, slot.calc, self.dcc)?;
        let fixed_accrual = (ck(yf) * self.fixed_rate * state.notional_principal).get()?;
        let floating_accrual =
            (ck(yf) * state.nominal_interest_rate * state.notional_principal).get()?;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                state.notional_principal = (ck(self.sgn) * self.notional).get()?;
                state.nominal_interest_rate = self.initial_floating_rate;
                state.accrued_interest = Decimal::ZERO;
                *floating = Decimal::ZERO;
                Decimal::ZERO
            }
            Kind::InterestPayment => {
                let payoff =
                    (ck(state.accrued_interest) + fixed_accrual - *floating - floating_accrual)
                        .get()?;
                state.accrued_interest = Decimal::ZERO;
                *floating = Decimal::ZERO;
                payoff
            }
            Kind::InterestPaymentFixed => {
                // `IPFL` follows on the same date with a zero year fraction,
                // so the floating accrual is carried forward first.
                *floating = (ck(*floating) + floating_accrual).get()?;
                let payoff = (ck(state.accrued_interest) + fixed_accrual).get()?;
                state.accrued_interest = Decimal::ZERO;
                payoff
            }
            Kind::InterestPaymentFloating => {
                state.accrued_interest = (ck(state.accrued_interest) + fixed_accrual).get()?;
                let payoff = (-(ck(*floating) + floating_accrual)).get()?;
                *floating = Decimal::ZERO;
                payoff
            }
            Kind::RateReset => {
                self.accrue(state, floating, fixed_accrual, floating_accrual)?;
                let code = terms
                    .market_object_code_of_rate_reset
                    .as_deref()
                    .ok_or(EngineError::MissingAttribute("marketObjectCodeOfRateReset"))?;
                let observed =
                    risk.rate(code, slot.calc)
                        .ok_or(EngineError::RiskFactorMissing {
                            code: code.to_string(),
                            at: slot.calc.to_string(),
                        })?;
                let multiplier = terms.rate_multiplier.unwrap_or(Decimal::ONE);
                let spread = terms.rate_spread.unwrap_or(Decimal::ZERO);
                state.nominal_interest_rate = (ck(observed) * multiplier + spread).get()?;
                Decimal::ZERO
            }
            Kind::Purchase => {
                self.accrue(state, floating, fixed_accrual, floating_accrual)?;
                let price = terms
                    .price_at_purchase_date
                    .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                (ck(self.sgn) * Decimal::NEGATIVE_ONE * price).get()?
            }
            Kind::Termination => {
                let price = terms
                    .price_at_termination_date
                    .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                self.close(state, floating, ContractStatus::Terminated);
                (ck(self.sgn) * price).get()?
            }
            Kind::Maturity => {
                self.close(state, floating, ContractStatus::Matured);
                Decimal::ZERO
            }
        };
        state.status_date = slot.calc;
        Ok(payoff)
    }

    /// Adds the period's leg accruals to `Ipac1` and `Ipac2`.
    fn accrue(
        &self,
        state: &mut ContractState,
        floating: &mut Decimal,
        fixed_accrual: Decimal,
        floating_accrual: Decimal,
    ) -> Result<(), EngineError> {
        state.accrued_interest = (ck(state.accrued_interest) + fixed_accrual).get()?;
        *floating = (ck(*floating) + floating_accrual).get()?;
        Ok(())
    }

    /// The `MD`/`TD` transition: no notional or accrual remains. The
    /// floating rate stays as reported by the testbed (SI-016).
    fn close(&self, state: &mut ContractState, floating: &mut Decimal, status: ContractStatus) {
        state.notional_principal = Decimal::ZERO;
        state.accrued_interest = Decimal::ZERO;
        *floating = Decimal::ZERO;
        state.contract_status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn close(actual: Decimal, expected: Decimal) {
        let diff = (actual - expected).abs();
        assert!(
            diff < Decimal::new(1, 9),
            "expected {expected}, actual {actual}"
        );
    }

    /// swppv02's shape: pay fix 5% on 1000, A365, quarterly interest from
    /// 2015-03-01, one reset to 3% on 2015-03-01; the float starts at
    /// `IPNR2` = 5%.
    fn pay_fix(settlement: &str) -> ContractTerms {
        terms(json!({
            "contractType": "SWPPV",
            "contractID": "swppv",
            "contractRole": "PF",
            "currency": "USD",
            "statusDate": "2014-12-30T00:00:00",
            "initialExchangeDate": "2015-01-01T00:00:00",
            "maturityDate": "2016-01-01T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.05",
            "nominalInterestRate2": "0.05",
            "dayCountConvention": "A365",
            "cycleAnchorDateOfInterestPayment": "2015-03-01T00:00:00",
            "cycleOfInterestPayment": "P3ML1",
            "cycleAnchorDateOfRateReset": "2015-03-01T00:00:00",
            "cycleOfRateReset": "P1YL1",
            "marketObjectCodeOfRateReset": "USD_Treasury",
            "deliverySettlement": settlement
        }))
    }

    fn treasury() -> StateProvider {
        StateProvider::new().with_rate("USD_Treasury", t("2015-03-01T00:00:00"), dec!(0.03))
    }

    fn summary(events: &[ContractEvent]) -> Vec<(EventType, NaiveDateTime)> {
        events.iter().map(|e| (e.event_type, e.time)).collect()
    }

    #[test]
    fn delivery_pays_the_fixed_and_floating_legs_separately() {
        let events = SwppvEngine
            .evaluate(&pay_fix("D"), &treasury())
            .expect("events");
        let first = &summary(&events)[..4];
        assert_eq!(
            first,
            [
                (EventType::InitialExchange, t("2015-01-01T00:00:00")),
                (EventType::InterestPaymentFixedLeg, t("2015-03-01T00:00:00")),
                (
                    EventType::InterestPaymentFloatingLeg,
                    t("2015-03-01T00:00:00")
                ),
                (EventType::RateResetVariable, t("2015-03-01T00:00:00")),
            ]
        );
        // No principal exchange; the float rate starts at IPNR2.
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, dec!(-1000));
        assert_eq!(events[0].state.nominal_interest_rate, dec!(0.05));
        // 59 days: pay 0.05 fixed, receive 0.05 floating.
        close(
            events[1].payoff,
            dec!(-1000) * dec!(0.05) * dec!(59) / dec!(365),
        );
        close(
            events[2].payoff,
            dec!(1000) * dec!(0.05) * dec!(59) / dec!(365),
        );
        assert_eq!(events[3].state.nominal_interest_rate, dec!(0.03));
        // 92 days at the reset rate.
        close(
            events[5].payoff,
            dec!(1000) * dec!(0.03) * dec!(92) / dec!(365),
        );
        let md = events.last().expect("MD");
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.payoff, Decimal::ZERO);
        assert_eq!(md.state.notional_principal, Decimal::ZERO);
        assert_eq!(md.state.nominal_interest_rate, dec!(0.03));
    }

    #[test]
    fn cash_settlement_nets_the_legs() {
        let events = SwppvEngine
            .evaluate(&pay_fix("S"), &treasury())
            .expect("events");
        let payments: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        assert_eq!(payments.len(), 5);
        // Equal rates net to zero; after the reset the payer of 5% fixed
        // against 3% floating pays 2% net.
        close(payments[0], Decimal::ZERO);
        close(payments[1], dec!(-1000) * dec!(0.02) * dec!(92) / dec!(365));
        assert!(!events
            .iter()
            .any(|e| e.event_type == EventType::InterestPaymentFixedLeg));
    }

    #[test]
    fn receive_fix_mirrors_pay_fix() {
        let mut receive = pay_fix("D");
        receive.contract_role = Some(ContractRole::Rf);
        let paid = SwppvEngine
            .evaluate(&pay_fix("D"), &treasury())
            .expect("events");
        let received = SwppvEngine.evaluate(&receive, &treasury()).expect("events");
        assert_eq!(paid.len(), received.len());
        for (p, r) in paid.iter().zip(&received) {
            assert_eq!(p.payoff, -r.payoff);
            assert_eq!(p.state.notional_principal, -r.state.notional_principal);
            assert_eq!(p.state.nominal_interest_rate, r.state.nominal_interest_rate);
        }
    }

    /// swppv13's shape: the purchase reports the fixed-leg accrual, the
    /// first `IPFL` after it pays the whole period, and nothing follows the
    /// termination.
    #[test]
    fn purchase_and_termination_bound_the_stream() {
        let mut swap = pay_fix("D");
        swap.purchase_date = Some(t("2015-03-15T23:59:59"));
        swap.price_at_purchase_date = Some(dec!(-600));
        swap.termination_date = Some(t("2015-07-01T00:00:00"));
        swap.price_at_termination_date = Some(dec!(400));
        let events = SwppvEngine.evaluate(&swap, &treasury()).expect("events");
        assert_eq!(
            events.iter().map(|e| e.event_type).collect::<Vec<_>>(),
            vec![
                EventType::Purchase,
                EventType::InterestPaymentFixedLeg,
                EventType::InterestPaymentFloatingLeg,
                EventType::Termination,
            ]
        );
        let prd = &events[0];
        assert_eq!(prd.payoff, dec!(-600));
        // 2015-03-01 to 2015-03-16 (23:59:59 counts to the next day).
        close(
            prd.state.accrued_interest,
            dec!(-1000) * dec!(0.05) * dec!(15) / dec!(365),
        );
        close(
            events[2].payoff,
            dec!(1000) * dec!(0.03) * dec!(92) / dec!(365),
        );
        let td = &events[3];
        assert_eq!(td.payoff, dec!(-400));
        assert_eq!(td.state.notional_principal, Decimal::ZERO);
        assert_eq!(td.state.contract_status, ContractStatus::Terminated);
    }

    #[test]
    fn leg_roles_are_synonyms_of_the_swap_roles() {
        let mut legacy = pay_fix("D");
        legacy.contract_role = Some(ContractRole::Pfl);
        assert_eq!(
            SwppvEngine.evaluate(&legacy, &treasury()).expect("PFL"),
            SwppvEngine
                .evaluate(&pay_fix("D"), &treasury())
                .expect("PF")
        );
    }

    #[test]
    fn invalid_contracts_are_reported() {
        let mut composed = pay_fix("D");
        composed.contract_structure = Some(Vec::new());
        assert!(matches!(
            SwppvEngine.evaluate(&composed, &treasury()),
            Err(EngineError::InvalidTransition(_))
        ));

        let mut buyer = pay_fix("D");
        buyer.contract_role = Some(ContractRole::Buy);
        assert!(matches!(
            SwppvEngine.evaluate(&buyer, &treasury()),
            Err(EngineError::InvalidTransition(_))
        ));

        let mut no_float = pay_fix("D");
        no_float.nominal_interest_rate2 = None;
        assert!(matches!(
            SwppvEngine.evaluate(&no_float, &treasury()),
            Err(EngineError::MissingAttribute("nominalInterestRate2"))
        ));

        assert!(matches!(
            SwppvEngine.evaluate(&pay_fix("D"), &StateProvider::new()),
            Err(EngineError::RiskFactorMissing { .. })
        ));
    }
}
