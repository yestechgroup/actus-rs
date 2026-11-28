//! Minimised inputs that used to panic (and so abort the WASM build), shared
//! by the native (`wasm_api.rs`) and the wasm-runtime (`wasm_runtime.rs`)
//! tests. Each must now come back as an error carrying the given message.
//!
//! Found by the never-panic fuzzing (`fuzz/`, `crates/actus-testgen`).

/// `(name, terms JSON, scenario JSON, expected error substring)`.
pub const CASES: &[(&str, &str, &str, &str)] = &[
    (
        // 1e21 x 1e9 x year fraction overflows Decimal in the IP accrual.
        "pam_accrual_overflow",
        r#"{"contractType":"PAM","contractID":"x","contractRole":"RPA",
            "statusDate":"2012-12-30T00:00:00","currency":"USD",
            "notionalPrincipal":"1000000000000000000000",
            "initialExchangeDate":"2013-01-01T00:00:00",
            "maturityDate":"2014-01-01T00:00:00","nominalInterestRate":"1000000000",
            "cycleAnchorDateOfInterestPayment":"2013-01-01T00:00:00",
            "cycleOfInterestPayment":"P1ML0","dayCountConvention":"A365"}"#,
        "{}",
        "decimal overflow",
    ),
    (
        // Without MD the maturity is ceil(NT / PRNXT) redemptions: PRNXT = 0
        // divided by zero.
        "lam_zero_prnxt_without_maturity",
        r#"{"contractType":"LAM","contractID":"x","contractRole":"RPA",
            "statusDate":"2012-12-30T00:00:00","currency":"USD",
            "notionalPrincipal":"5000","initialExchangeDate":"2013-01-01T00:00:00",
            "nominalInterestRate":"0.08","dayCountConvention":"A365",
            "cycleAnchorDateOfPrincipalRedemption":"2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption":"P1ML0","nextPrincipalRedemptionPayment":"0"}"#,
        "{}",
        "not redeemable",
    ),
    (
        "ann_zero_prnxt_without_maturity",
        r#"{"contractType":"ANN","contractID":"x","contractRole":"RPA",
            "statusDate":"2012-12-30T00:00:00","currency":"USD",
            "notionalPrincipal":"5000","initialExchangeDate":"2013-01-01T00:00:00",
            "nominalInterestRate":"0.08","dayCountConvention":"A365",
            "cycleAnchorDateOfPrincipalRedemption":"2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption":"P1ML0","nextPrincipalRedemptionPayment":"0"}"#,
        "{}",
        "not redeemable",
    ),
    (
        "lax_zero_prnxt_without_maturity",
        r#"{"contractType":"LAX","contractID":"x","contractRole":"RPA",
            "statusDate":"2012-12-30T00:00:00","currency":"USD",
            "notionalPrincipal":"5000","initialExchangeDate":"2013-01-01T00:00:00",
            "nominalInterestRate":"0.08","dayCountConvention":"A365",
            "cycleAnchorDateOfPrincipalRedemption":"2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption":"P1ML0","nextPrincipalRedemptionPayment":"0"}"#,
        "{}",
        "not redeemable",
    ),
    (
        // The agreed settlement XA / NT2 divided by a zero second notional.
        "fxout_zero_second_notional",
        r#"{"contractType":"FXOUT","contractID":"x","contractRole":"RPA",
            "statusDate":"2016-01-01T00:00:00","currency":"EUR","currency2":"USD",
            "notionalPrincipal":"1000000","notionalPrincipal2":"0",
            "exerciseAmount":"950000","initialExchangeDate":"2016-01-15T00:00:00",
            "maturityDate":"2016-07-15T00:00:00"}"#,
        "{}",
        "division by zero",
    ),
    (
        // quantity x price overflows in the STK purchase payoff.
        "stk_position_overflow",
        r#"{"contractType":"STK","contractID":"x","contractRole":"RPA",
            "statusDate":"2016-01-01T00:00:00","currency":"USD",
            "purchaseDate":"2016-01-15T00:00:00",
            "quantity":"79228162514264337593543950335","priceAtPurchaseDate":"2"}"#,
        "{}",
        "decimal overflow",
    ),
];
