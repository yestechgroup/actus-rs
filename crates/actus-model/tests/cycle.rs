//! Cycle term parsing tests.

use actus_model::cycle::{Cycle, CyclePeriod, CycleStub};
use actus_model::error::ModelError;

#[test]
fn parses_all_cycle_families() {
    let cases = [
        ("P1DL0", 1, CyclePeriod::Day, CycleStub::Long, 0),
        ("P27DL1", 27, CyclePeriod::Day, CycleStub::Long, 1),
        ("P29DL0", 29, CyclePeriod::Day, CycleStub::Long, 0),
        ("P2WL1", 2, CyclePeriod::Week, CycleStub::Long, 1),
        ("P1ML0", 1, CyclePeriod::Month, CycleStub::Long, 0),
        ("P6ML1", 6, CyclePeriod::Month, CycleStub::Long, 1),
        ("P10YL0", 10, CyclePeriod::Year, CycleStub::Long, 0),
    ];
    for (raw, length, period, stub, index) in cases {
        let cycle = Cycle::parse(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(cycle.length(), length);
        assert_eq!(cycle.period(), period);
        assert_eq!(cycle.stub(), stub);
        assert_eq!(cycle.index(), index);
        assert_eq!(cycle.to_string(), raw);
    }
}

#[test]
fn parses_short_and_undefined_stubs() {
    let short = Cycle::parse("P3MR1").unwrap();
    assert_eq!(short.stub(), CycleStub::Short);
    let undefined = Cycle::parse("P3MU1").unwrap();
    assert_eq!(undefined.stub(), CycleStub::Undefined);
}

#[test]
fn parse_is_case_insensitive_but_display_is_canonical() {
    let cycle = Cycle::parse("p1ml0").unwrap();
    assert_eq!(cycle.to_string(), "P1ML0");
}

#[test]
fn rejects_malformed_cycles() {
    for raw in [
        "P0M", "1ML0", "P1XL0", "", "P", "PM", "P1M", "P1ML", "P1ML12", "P0ML0", "PXML0",
    ] {
        let err = Cycle::parse(raw).expect_err(raw);
        assert!(
            matches!(err, ModelError::InvalidCycle(_)),
            "{raw} must be an InvalidCycle, got {err:?}"
        );
    }
}

#[test]
fn constructor_rejects_zero_length_and_multi_digit_index() {
    assert!(Cycle::new(0, CyclePeriod::Month, CycleStub::Long, 0).is_err());
    assert!(Cycle::new(1, CyclePeriod::Month, CycleStub::Long, 12).is_err());
    assert!(Cycle::new(1, CyclePeriod::Month, CycleStub::Long, 0).is_ok());
}

#[test]
fn serde_string_round_trip() {
    let cycle: Cycle = serde_json::from_value(serde_json::json!("P2ML1")).unwrap();
    assert_eq!(
        serde_json::to_value(cycle).unwrap(),
        serde_json::json!("P2ML1")
    );

    let err = serde_json::from_value::<Cycle>(serde_json::json!("P1XL0"));
    assert!(err.is_err());
}

#[test]
fn from_str_via_parse() {
    assert_eq!(
        "P1ML0".parse::<Cycle>().unwrap(),
        Cycle::parse("P1ML0").unwrap()
    );
}
