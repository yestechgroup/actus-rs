//! Contract events and event sequencing (ACTUS techspec sections
//! "Contract Events" and "Event Sequence").

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::EventType;

use crate::state::ContractState;

/// A single contract event with its payoff and post-event state.
///
/// Per the techspec, an event `e_t^k` carries an event time `t` and a payoff
/// `c` (the cash flow exchanged at `t` from the perspective of the contract
/// creator, role-signed); the accompanying post-event state snapshot makes
/// events self-contained for conformance reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractEvent {
    /// Event type `k`.
    pub event_type: EventType,
    /// Event time `t`.
    pub time: NaiveDateTime,
    /// Payoff `c` in the contract currency, role-signed.
    pub payoff: Decimal,
    /// Settlement currency of the payoff, when known.
    pub currency: Option<String>,
    /// Post-event contract state.
    pub state: ContractState,
}

/// Orders contract events into their deterministic evaluation sequence.
///
/// Per the techspec section "Event Sequence", events at the exact same time
/// are evaluated in the order given by the event sequence indicator of the
/// event dictionary: [`sequence_rank`]. Events are ordered by
/// `(time, sequence_rank)`; equal keys keep insertion order (stable sort), so
/// the ordering is a deterministic total order for any event list.
pub fn sort_events(events: &mut [ContractEvent]) {
    events.sort_by_key(|e| (e.time, sequence_rank(e.event_type)));
}

/// The effective same-timestamp sequence rank of an event type.
///
/// These are the [`EventType::priority`] dictionary values with two
/// deviations resolved against the official testbeds:
///
/// - The annuity principal fixing `PRF` evaluates after the same-timestamp
///   rate reset. The v1.4 dictionary assigns `PRF` sequence 5, but every ANN
///   fixture with a rate reset orders the same-day events
///   `PR, IP, RR/RRF, PRF` (ann15, ann16), because the fixing recalculates
///   `PRNXT` from the rate the reset has just set. The rank places `PRF`
///   directly after `RR`.
/// - The call money exercise notice `XD` evaluates before the same-timestamp
///   interest payment. The v1.4 dictionary assigns `XD` sequence 20, but the
///   CLM reference implementation emits the terminal call settlement in the
///   order `XD, IP, STD` at the shared settlement timestamp (clm09, clm13,
///   clm14), because the notice freezes the accrual the interest payment
///   then carries. The rank places `XD` directly before `IP`; no other
///   contract family emits `XD`.
pub fn sequence_rank(event_type: EventType) -> u8 {
    match event_type {
        EventType::PrincipalPaymentAmountFixing => EventType::RateResetVariable.priority() + 1,
        EventType::Exercise => EventType::InterestPayment.priority() - 1,
        other => other.priority(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ContractState;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn event(event_type: EventType, time: NaiveDateTime, payoff: Decimal) -> ContractEvent {
        ContractEvent {
            event_type,
            time,
            payoff,
            currency: Some("USD".to_string()),
            state: ContractState::default(),
        }
    }

    #[test]
    fn same_timestamp_orders_by_dictionary_sequence() {
        let mut events = vec![
            event(
                EventType::InterestPayment,
                t("2014-01-01T00:00:00"),
                dec!(25),
            ),
            event(EventType::Maturity, t("2014-01-01T00:00:00"), dec!(3000)),
            event(
                EventType::RateResetVariable,
                t("2014-01-01T00:00:00"),
                Decimal::ZERO,
            ),
        ];
        sort_events(&mut events);
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![
                EventType::InterestPayment,
                EventType::RateResetVariable,
                EventType::Maturity
            ]
        );
    }

    #[test]
    fn initial_exchange_sorts_before_maturity_at_same_timestamp() {
        let mut events = vec![
            event(EventType::Maturity, t("2014-01-01T00:00:00"), dec!(3000)),
            event(
                EventType::InitialExchange,
                t("2014-01-01T00:00:00"),
                dec!(-3000),
            ),
        ];
        sort_events(&mut events);
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(types, vec![EventType::InitialExchange, EventType::Maturity]);
    }

    #[test]
    fn ordering_is_time_first_priority_second() {
        let mut events = vec![
            event(EventType::Maturity, t("2014-01-01T00:00:00"), dec!(3000)),
            event(
                EventType::InterestPayment,
                t("2013-01-01T00:00:00"),
                dec!(25),
            ),
        ];
        sort_events(&mut events);
        assert_eq!(events[0].time, t("2013-01-01T00:00:00"));
        assert_eq!(events[0].event_type, EventType::InterestPayment);
        assert_eq!(events[1].event_type, EventType::Maturity);
    }

    #[test]
    fn equal_keys_keep_insertion_order() {
        let mut events = vec![
            event(
                EventType::InterestPayment,
                t("2013-01-01T00:00:00"),
                dec!(1),
            ),
            event(
                EventType::InterestPayment,
                t("2013-01-01T00:00:00"),
                dec!(2),
            ),
        ];
        sort_events(&mut events);
        assert_eq!(events[0].payoff, dec!(1));
        assert_eq!(events[1].payoff, dec!(2));
    }

    #[test]
    fn sort_is_total_and_deterministic() {
        let build = || {
            vec![
                event(EventType::Maturity, t("2014-06-01T00:00:00"), dec!(9)),
                event(
                    EventType::PrincipalRedemption,
                    t("2014-01-01T00:00:00"),
                    dec!(3),
                ),
                event(
                    EventType::InterestPayment,
                    t("2014-01-01T00:00:00"),
                    dec!(2),
                ),
                event(
                    EventType::InitialExchange,
                    t("2013-01-01T00:00:00"),
                    dec!(1),
                ),
                event(
                    EventType::RateResetVariable,
                    t("2014-01-01T00:00:00"),
                    dec!(4),
                ),
            ]
        };
        let mut first = build();
        let mut second = build();
        sort_events(&mut first);
        sort_events(&mut second);
        assert_eq!(first, second);
        let sequence: Vec<(NaiveDateTime, u8)> = first
            .iter()
            .map(|e| (e.time, e.event_type.priority()))
            .collect();
        let mut sorted = sequence.clone();
        sorted.sort();
        assert_eq!(sequence, sorted);
    }
}
