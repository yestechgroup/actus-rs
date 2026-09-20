//! Server-side rendering state for the ACTUS Explorer: the contract type
//! catalogue, per-type parameter forms, the applicability matrix and the
//! precomputed event/chart views of the selected contract.
//!
//! The same WASM crate code paths ([`actus_wasm::dto`], [`actus_wasm::specs`])
//! feed both this SSR state and the browser client, so the initial server
//! render and the hydrated client agree by construction.

use std::collections::{BTreeMap, HashSet};

use actus_model::ContractType;
use actus_wasm::dto::{self, EventDto};
use actus_wasm::specs::{self, ParamSpec};
use serde_json::{json, Value};

/// Metadata of one contract type as shown in the sidebar and header.
#[derive(Debug, Clone)]
pub struct TypeEntry {
    pub acronym: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub spec: String,
    pub key_events: Vec<&'static str>,
    pub has_engine: bool,
}

/// Static descriptions of the engine-backed types (mirroring the issue #1
/// prototype); other types fall back to a generated description.
#[allow(clippy::type_complexity)]
fn type_descriptions(
) -> BTreeMap<String, (&'static str, &'static str, &'static str, Vec<&'static str>)> {
    [
        ("PAM", ("Principal At Maturity", "A bullet loan where the full principal is repaid at maturity. Interest is paid periodically during the contract lifetime.", "The most fundamental debt contract. The lender provides the notional amount at IED, the borrower pays periodic interest (IP events) and repays the full principal at MD. Supports fixed and variable rates (RR events), fee accrual, and principal payments (PP) during lifetime.", vec!["IED", "IP", "MD"])),
        ("LAM", ("Linear Amortizer", "Principal is repaid in equal installments at regular intervals, with interest calculated on the outstanding balance.", "Similar to PAM but with regular principal redemption (PR) events. Each payment reduces the outstanding notional by a fixed amount (PRNXT), resulting in decreasing interest payments over time. Total payment amount decreases each period.", vec!["IED", "IP", "PR", "MD"])),
        ("NAM", ("Negative Amortizer", "Principal payments may be less than accrued interest, causing the outstanding notional to increase over time.", "When scheduled principal payments are insufficient to cover accrued interest, the unpaid interest is capitalized (added to principal). Can result in negative amortization where the outstanding balance grows.", vec!["IED", "IP", "PR", "MD"])),
        ("ANN", ("Annuity", "Fixed total periodic payment combining principal and interest. Interest portion decreases while principal portion increases over time.", "The annuity amount A is computed such that the total payment remains constant across periods. Early payments are mostly interest; later payments are mostly principal. Uses the annuity formula: A = (N+a)\u{b7}\u{3a0}(1+r\u{b7}Y) / (1+\u{3a3}\u{3a0}(1+r\u{b7}Y)).", vec!["IED", "IP", "PR", "MD"])),
        ("CSH", ("Cash", "Simple cash position. Represents a static amount of money held.", "The simplest ACTUS contract - a single cash position with no future events beyond the initial state. Used as a building block in composed contracts.", vec!["IED"])),
        ("CLM", ("Call Money", "Interbank overnight lending with undefined maturity. Either party can terminate at any time.", "Used for overnight and short-term interbank lending. No fixed maturity date - the contract ends when either party calls it (PR event). Interest accrues daily and is paid at termination.", vec!["IED", "IP", "MD"])),
        ("SWAPS", ("Generic Swap", "Generic swap combining any two leg contracts with potentially different characteristics.", "The most general swap contract. Combines a FirstLeg and SecondLeg, each of which can be any ACTUS contract. Congruent events (same type and time) are merged into aggregate events. Enables complex multi-leg structures.", vec!["IED", "IP", "MD"])),
        ("CEC", ("Credit Enhancement Collateral", "Collateral backing a credit enhancement guarantee. Value is the minimum of collateral market value and guaranteed exposure.", "Represents collateral assets pledged to back a CEG. The collateral value is min(\u{3a3} collateral market values, CECV \u{d7} guaranteed exposure). Settles at STD when the guarantee is triggered or matures.", vec!["IED", "MD", "STD"])),
        ("LAX", ("Exotic Linear Amortizer", "Linear amortizer with array-based schedules allowing irregular principal payments and varying cycles.", "Extends LAM with vector-valued attributes (ARPRANX, ARPRCL, ARINCDEC) enabling custom principal redemption schedules. Supports both increasing (INC) and decreasing (DEC) principal patterns across different periods.", vec!["IED", "IP", "PR", "MD"])),
        ("UMP", ("Undefined Maturity Profile", "Cash account with undefined maturity. Principal and interest movements occur via unscheduled events.", "Represents savings accounts, current accounts, and other products where cash flows occur at irregular intervals. All events are driven by observed unscheduled PR (principal) and PI (interest) events.", vec!["IED", "IP"])),
        ("STK", ("Stock", "Equity instrument with optional periodic dividend payments.", "Represents ownership in an entity. Payoffs are driven by market observations (stock price). Supports fixed dividends (DVNP) and variable dividends based on market rates.", vec!["IED", "DV"])),
        ("COM", ("Commodity", "Physical or synthetic commodity holding with market-driven valuation.", "Represents holdings in commodities (oil, metals, agricultural products). Similar to STK but for physical assets. Value is driven by market observations of the commodity price.", vec!["IED"])),
        ("FXOUT", ("FX Outright", "Foreign exchange transaction exchanging two currencies at a future date at a predetermined rate.", "An outright FX forward contract. At STD (settlement date), one currency is exchanged for another at an agreed rate. Payoff depends on the difference between contract rate and market rate at settlement.", vec!["IED", "STD"])),
        ("SWPPV", ("Plain Vanilla Swap", "Interest rate swap exchanging fixed-rate payments for floating-rate payments.", "The most common interest rate derivative. One party pays fixed rate (IPNR), the other pays floating rate (observed via RRMO). Net settlement occurs at each IP date. Used for hedging interest rate risk.", vec!["IED", "IP", "MD"])),
        ("CAPFL", ("Cap-Floor", "Interest rate option that pays when reference rate exceeds a cap or falls below a floor.", "Combines a cap (upper rate limit RRLC) and floor (lower rate limit RRLF) on an underlying interest rate contract. Payoff is the absolute difference between the capped/floored and uncapped cash flows.", vec!["IED", "IP", "MD"])),
        ("OPTNS", ("Option", "Right but not obligation to buy (call) or sell (put) an underlying at a specified strike price.", "Standard option contract. At exercise date (XD), holder can exercise if in-the-money. Call options (OPTP=C) profit when St > OPS1 (strike). Put options (OPTP=P) profit when St < OPS1. Payoff is max(\u{b1}(St-OPS1), 0).", vec!["IED", "XD", "MD"])),
        ("FUTUR", ("Future", "Obligation to buy or sell an underlying at a predetermined forward price at a future date.", "Futures contract with payoff St - PFUT at maturity, where St is the spot price and PFUT is the forward price. Unlike options, futures create an obligation for both parties.", vec!["IED", "MD"])),
        ("CEG", ("Credit Enhancement Guarantee", "Guarantee contract covering credit events on one or more underlying contracts.", "Credit enhancement providing protection against credit events (CE) on covered contracts. Guarantor pays when covered contracts default. Exposure depends on coverage ratio (CECVR) and guarantee type (CEGE: NI=non-integrated, NO=notional).", vec!["IED", "FP", "MD"])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// The catalogue of all contract types with engine availability.
#[must_use]
pub fn type_entries() -> Vec<TypeEntry> {
    let descriptions = type_descriptions();
    actus_model::metadata::contract_types()
        .into_iter()
        .map(|info| {
            let parsed: Result<ContractType, _> = info.acronym.parse();
            let has_engine = parsed.is_ok_and(dto::engine_supports);
            let default = (
                info.identifier.as_str(),
                "ACTUS contract type of the ACTUS Reference Implementation dictionary.",
                "ACTUS contract type of the ACTUS Reference Implementation dictionary.",
                Vec::new(),
            );
            let (name, description, spec, key_events) =
                descriptions.get(&info.acronym).cloned().unwrap_or(default);
            TypeEntry {
                acronym: info.acronym,
                name: name.to_string(),
                category: info.category.unwrap_or_else(|| "Basic".to_string()),
                description: description.to_string(),
                spec: spec.to_string(),
                key_events,
                has_engine,
            }
        })
        .collect()
}

/// One sidebar entry with precomputed flags for template binding.
fn sidebar_entry(entry: &TypeEntry) -> Value {
    json!({
        "acronym": entry.acronym,
        "name": entry.name,
        "category": entry.category,
        "description": entry.description,
        "hasEngine": entry.has_engine,
        "isBasic": entry.category == "Basic",
        "isCombined": entry.category == "Combined",
        "isCredit": entry.category == "Credit Enhancement",
    })
}

/// One parameter field of the live form, with validation status.
#[derive(Debug, Clone)]
pub struct ParamView {
    pub spec: ParamSpec,
    pub required: bool,
    pub set: bool,
    pub status: String,
    pub invalid: bool,
    pub error_code: String,
}

/// Builds the live parameter view for one type from form values.
#[must_use]
pub fn param_views(
    contract_type: ContractType,
    values: &BTreeMap<String, String>,
    report: &dto::ValidationReport,
) -> Vec<ParamView> {
    let tables = actus_model::generated::applicability::tables(contract_type);
    specs::param_specs(contract_type)
        .into_iter()
        .map(|spec| {
            let value = values.get(&spec.key).cloned().unwrap_or_default();
            let set = !value.trim().is_empty();
            let required = tables.base_required.contains(&spec.key.as_str());
            let status = report
                .term_status
                .get(&spec.key)
                .cloned()
                .unwrap_or_else(|| {
                    match (required, set) {
                        (true, true) => "required-set",
                        (true, false) => "required-missing",
                        (false, true) => "optional-set",
                        (false, false) => "optional-unset",
                    }
                    .to_string()
                });
            let error_code = report
                .errors
                .iter()
                .find(|e| e.attribute == spec.key)
                .map(|e| e.code.clone())
                .unwrap_or_default();
            ParamView {
                spec,
                required,
                set,
                status,
                invalid: !error_code.is_empty(),
                error_code,
            }
        })
        .collect()
}

/// One rendered timeline marker / table row of an event.
#[must_use]
pub fn event_view(event: &EventDto, index: usize, total: usize, min: i64, range: i64) -> Value {
    let time = chrono::NaiveDateTime::parse_from_str(&event.event_date, dto::TIMESTAMP_FORMAT)
        .map(|t| t.and_utc().timestamp())
        .unwrap_or(0);
    let ratio = if range > 0 {
        f64::from((time - min).clamp(0, range) as i32) / f64::from(range as i32)
    } else {
        0.0
    };
    let left = 4.0 + 92.0 * ratio;
    let payoff: f64 = event.payoff.parse().unwrap_or(0.0);
    json!({
        "dateText": event.event_date,
        "eventType": event.event_type,
        "payoffText": format_money_short(payoff, true),
        "notionalText": format_money_short(event.notional_principal.parse().unwrap_or(0.0), false),
        "leftPct": format!("{left:.2}"),
        "kind": event_kind(&event.event_type),
        "side": if index.is_multiple_of(2) { "top" } else { "bottom" },
        "sign": if payoff > 0.0 { "in" } else if payoff < 0.0 { "out" } else { "zero" },
        "isLast": index + 1 == total,
    })
}

/// The event type color family of the issue #1 prototype.
#[must_use]
pub fn event_kind(event_type: &str) -> &'static str {
    match event_type {
        "IED" => "ied",
        "IP" | "IPCI" => "ip",
        "PR" | "PP" | "PY" | "PI" => "pr",
        "MD" => "md",
        _ => "other",
    }
}

/// The swim-lane timeline view: one lane per event family, year gridlines,
/// and a scroll axis that scales with the event count. Mirrors
/// `buildTimeline` in `assets/src/view.ts` — both feed the same template.
#[must_use]
pub fn timeline_view(events: &[EventDto], min: i64, range: i64) -> Value {
    const LANES: [(&str, &str); 5] = [
        ("ied", "Initial exchange"),
        ("ip", "Interest"),
        ("pr", "Principal"),
        ("md", "Maturity"),
        ("other", "Other"),
    ];
    let left_pct = |time: i64| -> String {
        let ratio = if range > 0 {
            f64::from((time - min).clamp(0, range) as i32) / f64::from(range as i32)
        } else {
            0.0
        };
        format!("{:.2}", 4.0 + 92.0 * ratio)
    };
    let lanes: Vec<Value> = LANES
        .iter()
        .map(|(key, label)| {
            let lane_events: Vec<Value> = events
                .iter()
                .filter(|e| event_kind(&e.event_type) == *key)
                .map(|e| {
                    let payoff: f64 = e.payoff.parse().unwrap_or(0.0);
                    json!({
                        "dateText": e.event_date,
                        "eventType": e.event_type,
                        "eventLabel": e.event_type,
                        "payoffText": format_money_short(payoff, true),
                        "notionalText": format_money_short(e.notional_principal.parse().unwrap_or(0.0), false),
                        "leftPct": left_pct(timestamp_of(e)),
                        "kind": event_kind(&e.event_type),
                        "sign": if payoff > 0.0 { "in" } else if payoff < 0.0 { "out" } else { "zero" },
                    })
                })
                .collect();
            json!({
                "key": key,
                "label": label,
                "hasLane": !lane_events.is_empty(),
                "events": lane_events,
            })
        })
        .collect();
    let axis_min_width = (events.len() * 30).clamp(900, 6400);
    json!({
        "hasEvents": !events.is_empty(),
        "eventCount": events.len(),
        "axisMinWidth": format!("{axis_min_width}px"),
        "yearTicks": year_ticks(min, range, &left_pct),
        "lanes": lanes,
    })
}

fn timestamp_of(event: &EventDto) -> i64 {
    chrono::NaiveDateTime::parse_from_str(&event.event_date, dto::TIMESTAMP_FORMAT)
        .map(|t| t.and_utc().timestamp())
        .unwrap_or(0)
}

/// Maximum number of month cards the calendar view renders before it
/// truncates (dense long-dated contracts would otherwise explode the DOM).
const CALENDAR_MAX_MONTHS: usize = 120;

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The calendar schedule view: month cards grouped by year, event days
/// marked with per-family dots and a hover detail card. Mirrors
/// `buildCalendar` in `assets/src/view.ts`.
#[must_use]
pub fn calendar_view(events: &[EventDto], grace_days: Option<i64>) -> Value {
    if events.is_empty() {
        return json!({ "hasEvents": false, "truncated": false, "notice": "", "years": [] });
    }
    // Group events by (year, month, day).
    let mut by_date: BTreeMap<(i32, u32, u32), Vec<&EventDto>> = BTreeMap::new();
    for event in events {
        if let Ok(t) =
            chrono::NaiveDateTime::parse_from_str(&event.event_date, dto::TIMESTAMP_FORMAT)
        {
            use chrono::Datelike;
            by_date
                .entry((t.year(), t.month(), t.day()))
                .or_default()
                .push(event);
        }
    }
    // Days that fall into a grace window: for every event, the `grace_days`
    // days AFTER the due date (the due day itself is styled as the event).
    let grace_set: HashSet<(i32, u32, u32)> = match grace_days {
        Some(g) if g > 0 => {
            let mut set = HashSet::new();
            for key in by_date.keys() {
                let Some(due) = chrono::NaiveDate::from_ymd_opt(key.0, key.1, key.2) else {
                    continue;
                };
                for offset in 1..=g {
                    let Some(day) = due.checked_add_signed(chrono::Duration::days(offset)) else {
                        break;
                    };
                    use chrono::Datelike;
                    set.insert((day.year(), day.month(), day.day()));
                }
            }
            set
        }
        _ => HashSet::new(),
    };
    let first_date = by_date.keys().next().map(|(y, m, _)| (*y, *m));
    let last_date = by_date.keys().next_back().map(|(y, m, _)| (*y, *m));
    let Some((first_year, first_month)) = first_date else {
        return json!({ "hasEvents": false, "truncated": false, "notice": "", "years": [] });
    };
    let Some((last_year, last_month)) = last_date else {
        return json!({ "hasEvents": false, "truncated": false, "notice": "", "years": [] });
    };
    let mut years: Vec<Value> = Vec::new();
    let mut rendered_months = 0usize;
    let mut truncated = false;
    let mut current_year: Option<i32> = None;
    let mut current_months: Vec<Value> = Vec::new();
    let mut cursor = (first_year, first_month);
    loop {
        if rendered_months >= CALENDAR_MAX_MONTHS {
            truncated = true;
            break;
        }
        let (year, month) = cursor;
        if current_year != Some(year) {
            if let Some(y) = current_year.take() {
                years.push(json!({ "year": y, "label": y.to_string(), "months": current_months }));
                current_months = Vec::new();
            }
            current_year = Some(year);
        }
        current_months.push(month_view(year, month, &by_date, &grace_set));
        rendered_months += 1;
        if cursor == (last_year, last_month) {
            break;
        }
        // Advance one calendar month.
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        cursor = (ny, nm);
    }
    if let Some(y) = current_year.take() {
        years.push(json!({ "year": y, "label": y.to_string(), "months": current_months }));
    }
    let notice = if truncated {
        let first_month_label = format!("{first_year}-{first_month:02}");
        format!(
            "Showing the first {CALENDAR_MAX_MONTHS} months from {first_month_label} — adjust the term dates to explore later periods."
        )
    } else {
        String::new()
    };
    json!({
        "hasEvents": true,
        "truncated": truncated,
        "notice": notice,
        "years": years,
    })
}

/// One month card of the calendar.
fn month_view(
    year: i32,
    month: u32,
    by_date: &BTreeMap<(i32, u32, u32), Vec<&EventDto>>,
    grace_set: &HashSet<(i32, u32, u32)>,
) -> Value {
    use chrono::Datelike;
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1);
    let offset = first
        .map(|d| d.weekday().num_days_from_monday() as usize)
        .unwrap_or(0);
    let days_in_month = first
        .map(|_| {
            let (ny, nm) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            (chrono::NaiveDate::from_ymd_opt(ny, nm, 1))
                .and_then(|n| n.pred_opt())
                .map(|p| p.day())
                .unwrap_or(30)
        })
        .unwrap_or(30);
    let empty = Vec::new();
    let mut cells: Vec<Value> = Vec::new();
    for _ in 0..offset {
        cells.push(json!({ "blank": true }));
    }
    for day in 1..=days_in_month {
        let day_events = by_date.get(&(year, month, day)).unwrap_or(&empty);
        let in_grace = !day_events.is_empty() || grace_set.contains(&(year, month, day));
        if day_events.is_empty() {
            cells.push(json!({
                "blank": false,
                "day": day,
                "hasEvent": false,
                "grace": in_grace,
            }));
            continue;
        }
        // Compact type badges: up to two explicit chips, then a "+n" chip.
        let badges: Vec<Value> = day_events
            .iter()
            .take(2)
            .map(|e| json!({ "type": e.event_type, "kind": event_kind(&e.event_type) }))
            .collect();
        let more = day_events.len().saturating_sub(2);
        let lines: Vec<Value> = day_events
            .iter()
            .map(|e| {
                json!({
                    "eventType": e.event_type,
                    "payoffText": format_money_short(e.payoff.parse().unwrap_or(0.0), true),
                })
            })
            .collect();
        cells.push(json!({
            "blank": false,
            "day": day,
            "hasEvent": true,
            "grace": in_grace,
            "kind": event_kind(&day_events[0].event_type),
            "badges": badges,
            "more": more,
            "lines": lines,
        }));
    }
    json!({
        "key": format!("{year}-{month:02}"),
        "label": MONTH_NAMES[(month.clamp(1, 12) - 1) as usize],
        "hasEvents": first.map(|d| month_has_events(d, by_date)).unwrap_or(false),
        "cells": cells,
    })
}

fn month_has_events(
    first_day: chrono::NaiveDate,
    by_date: &BTreeMap<(i32, u32, u32), Vec<&EventDto>>,
) -> bool {
    use chrono::Datelike;
    by_date
        .keys()
        .any(|(y, m, _)| *y == first_day.year() && *m == first_day.month())
}

/// Parses an ISO 8601 period string (`P10D`, `P2M`, `P1Y`, multi-component
/// like `P1M15D`) into an approximate day count (month = 30, year = 365),
/// for calendar grace-window shading.
#[must_use]
pub fn parse_iso_period_days(raw: Option<&String>) -> Option<i64> {
    let raw = raw?.trim();
    let rest = raw.strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    let mut matched = 0usize;
    let bytes = rest.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let digits_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start || i >= bytes.len() {
            return None;
        }
        let n: i64 = rest[digits_start..i].parse().ok()?;
        let factor = match bytes[i] {
            b'D' => 1,
            b'W' => 7,
            b'M' => 30,
            b'Y' => 365,
            _ => return None,
        };
        total += n.checked_mul(factor)?;
        matched += i - digits_start + 1;
        i += 1;
    }
    (matched == rest.len()).then_some(total)
}

/// January-first gridlines, at most ~12, evenly stepped over the range.
fn year_ticks(min: i64, range: i64, left_pct: &impl Fn(i64) -> String) -> Vec<Value> {
    let Some(first) = chrono::DateTime::from_timestamp(min, 0) else {
        return Vec::new();
    };
    let Some(last) = chrono::DateTime::from_timestamp(min + range, 0) else {
        return Vec::new();
    };
    let y0 = chrono::Datelike::year(&first) + 1;
    let y1 = chrono::Datelike::year(&last);
    if y1 < y0 {
        return Vec::new();
    }
    let step = (((y1 - y0 + 1) as f64) / 12.0).ceil().max(1.0) as i32;
    (y0..=y1)
        .step_by(step as usize)
        .filter_map(|year| {
            let jan1 = chrono::NaiveDate::from_ymd_opt(year, 1, 1)?
                .and_hms_opt(0, 0, 0)?
                .and_utc()
                .timestamp();
            (jan1 > min && jan1 <= min + range)
                .then(|| json!({ "leftPct": left_pct(jan1), "label": year.to_string() }))
        })
        .collect()
}

/// Compact money label ("1.20M") used by the timeline and table.
#[must_use]
pub fn format_money_short(value: f64, signed: bool) -> String {
    let magnitude = value.abs();
    let text = if magnitude >= 1_000_000_000.0 {
        format!("{:.2}B", magnitude / 1_000_000_000.0)
    } else if magnitude >= 1_000_000.0 {
        format!("{:.2}M", magnitude / 1_000_000.0)
    } else if magnitude >= 1_000.0 {
        format!("{:.2}K", magnitude / 1_000.0)
    } else if magnitude == 0.0 {
        "\u{2014}".to_string()
    } else {
        format!("{magnitude:.2}")
    };
    if magnitude == 0.0 {
        return "\u{2014}".to_string();
    }
    let sign = if value < 0.0 {
        "\u{2212}"
    } else if signed {
        "+"
    } else {
        ""
    };
    format!("{sign}{text}")
}

/// The four headline stat cards of the selected contract.
#[must_use]
pub fn stats_view(events: &[EventDto]) -> Value {
    let mut total_interest = 0.0;
    let mut total_principal = 0.0;
    for event in events {
        let payoff: f64 = event.payoff.parse().unwrap_or(0.0);
        match event_kind(&event.event_type) {
            "ip" => total_interest += payoff.abs(),
            "pr" | "md" => total_principal += payoff.abs(),
            _ => {}
        }
    }
    let final_notional = events
        .last()
        .map(|e| e.notional_principal.parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);
    let tone = |v: f64| if v >= 0.0 { "good" } else { "bad" };
    json!({
        "cards": [
            { "label": "Total Interest", "value": format_money_short(total_interest, false), "tone": "good" },
            { "label": "Total Principal", "value": format_money_short(total_principal, false), "tone": tone(-total_principal), "absolute": true },
            { "label": "Events", "value": events.len().to_string(), "tone": "neutral" },
            { "label": "Final Notional", "value": format_money_short(final_notional, false), "tone": tone(final_notional) },
        ],
        "hasCards": !events.is_empty(),
    })
}

/// The precomputed cash-flow chart geometry (bar heights, polyline points,
/// axis ticks). Percentages are 0-100 strings, points are SVG polyline
/// coordinate strings on a fixed 0 0 1000 300 viewBox.
#[must_use]
pub fn chart_view(events: &[EventDto]) -> Value {
    if events.is_empty() {
        return json!({ "hasData": false, "bars": [], "linePoints": "", "cumulativePoints": "", "yTicks": [] });
    }
    let payoffs: Vec<f64> = events
        .iter()
        .map(|e| e.payoff.parse().unwrap_or(0.0))
        .collect();
    let max_abs = payoffs.iter().fold(0.0_f64, |a, v| a.max(v.abs())).max(1.0);
    let n = payoffs.len().max(2);
    let step = 1000.0 / f64::from((n - 1) as u32);
    let mut cumulative = 0.0;
    let mut cum_values = Vec::with_capacity(payoffs.len());
    let mut bars = Vec::with_capacity(payoffs.len());
    for payoff in &payoffs {
        cumulative += payoff;
        cum_values.push(cumulative);
        let height = (payoff.abs() / max_abs * 100.0).clamp(2.0, 100.0);
        bars.push(json!({
            "heightPct": format!("{height:.2}"),
            "sign": if *payoff > 0.0 { "in" } else if *payoff < 0.0 { "out" } else { "zero" },
            "label": format_money_short(*payoff, true),
        }));
    }
    let cum_max = cum_values
        .iter()
        .fold(0.0_f64, |a, v| a.max(v.abs()))
        .max(1.0);
    let points = |values: &[f64], bound: f64| -> String {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = step * f64::from(i as u32);
                let y = 300.0 - 20.0 - 260.0 * ((v / bound).clamp(-1.0, 1.0) + 1.0) / 2.0;
                format!("{x:.1},{y:.1}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut y_ticks = Vec::new();
    for share in [1.0, 0.5, 0.0, -0.5, -1.0] {
        let pct = (share + 1.0) / 2.0 * 100.0;
        y_ticks.push(json!({
            "bottomPct": format!("{pct:.1}"),
            "label": format_money_short(max_abs * share, true),
        }));
    }
    json!({
        "hasData": true,
        "bars": bars,
        "linePoints": points(&payoffs, max_abs),
        "cumulativePoints": points(&cum_values, cum_max),
        "yTicks": y_ticks,
    })
}

/// The applicability heatmap: one row per contract type, one cell per
/// dictionary attribute (`required` | `base` | `applicable` | `none`).
#[must_use]
pub fn matrix_view(selected: &str) -> Value {
    use actus_model::generated::attribute::{self, AttributeType};

    let type_name = |t: AttributeType| match t {
        AttributeType::Boolean => "Boolean",
        AttributeType::ContractReferenceArray => "ContractReference[]",
        AttributeType::Cycle => "Cycle",
        AttributeType::CycleArray => "Cycle[]",
        AttributeType::Enum => "Enum",
        AttributeType::EnumArray => "Enum[]",
        AttributeType::Period => "Period",
        AttributeType::Real => "Real",
        AttributeType::RealArray => "Real[]",
        AttributeType::Text => "Text",
        AttributeType::Timestamp => "Timestamp",
        AttributeType::TimestampArray => "Timestamp[]",
    };
    let attributes: Vec<Value> = attribute::ALL
        .iter()
        .map(|a| {
            json!({
                "acronym": a.acronym,
                "identifier": a.identifier,
                "name": a.name,
                "description": a.description,
                "dataType": type_name(a.attribute_type),
            })
        })
        .collect();
    let mut rows = Vec::new();
    for entry in dto::applicability_matrix() {
        let applicable: std::collections::BTreeSet<&str> =
            entry.tables.applicable.iter().map(String::as_str).collect();
        let required: std::collections::BTreeSet<&str> =
            entry.tables.required.iter().map(String::as_str).collect();
        let base: std::collections::BTreeSet<&str> = entry
            .tables
            .base_required
            .iter()
            .map(String::as_str)
            .collect();
        let mut cells = Vec::with_capacity(attribute::ALL.len());
        for a in attribute::ALL {
            let level = if a.identifier == "contractType" {
                "applicable"
            } else if base.contains(a.identifier) {
                "base"
            } else if required.contains(a.identifier) {
                "required"
            } else if applicable.contains(a.identifier) {
                "applicable"
            } else {
                "none"
            };
            cells.push(level);
        }
        let acronym = entry.acronym;
        rows.push(json!({
            "acronym": acronym,
            "isSelected": acronym == selected,
            "cells": cells,
        }));
    }
    json!({
        "attrCount": attribute::ALL.len(),
        "attributes": attributes,
        "rows": rows,
    })
}

/// The complete SSR render state for the explorer page.
#[must_use]
pub fn build_state(selected_acronym: &str) -> Value {
    let entries = type_entries();
    let selected: Result<ContractType, _> = selected_acronym.parse();
    let selected_type = selected.unwrap_or(ContractType::Pam);
    let selected_entry = entries
        .iter()
        .find(|e| e.acronym == selected_type.as_acronym())
        .cloned()
        .expect("PAM exists in the catalogue");

    // Form values and validation for the selected type.
    let values = specs::default_values(selected_type);
    let terms = specs::terms_json(selected_type, &values);
    let terms_raw = serde_json::to_string(&terms).unwrap_or_else(|_| "{}".to_string());
    let report: dto::ValidationReport = serde_json::from_str(
        &dto::validate_json(&terms_raw)
            .unwrap_or_else(|_| "{\"valid\":true,\"errors\":[],\"termStatus\":{}}".to_string()),
    )
    .unwrap_or(dto::ValidationReport {
        valid: true,
        errors: Vec::new(),
        term_status: BTreeMap::new(),
    });
    let params: Vec<Value> = param_views(selected_type, &values, &report)
        .iter()
        .map(|p| {
            let (label, code) = split_label(&p.spec.label);
            json!({
                "key": p.spec.key,
                "label": label,
                "code": code,
                "kind": p.spec.kind,
                "value": p.spec.value,
                "step": p.spec.step,
                "options": p.spec.options,
                "required": p.required,
                "set": p.set,
                "status": p.status,
                "invalid": p.invalid,
                "errorCode": p.error_code,
            })
        })
        .collect();

    // Evaluation through the shared WASM code path.
    let evaluation = dto::evaluate_json(&terms_raw)
        .ok()
        .and_then(|raw| serde_json::from_str::<dto::EvaluationResult>(&raw).ok());
    let engine_notice = if !selected_entry.has_engine {
        "Engine not yet available for this contract type \u{2014} showing the parameter form only."
            .to_string()
    } else {
        String::new()
    };
    let events = evaluation.map(|e| e.events).unwrap_or_default();
    let (min, range) = event_bounds(&events);
    let event_views: Vec<Value> = events
        .iter()
        .enumerate()
        .map(|(i, e)| event_view(e, i, events.len(), min, range))
        .collect();
    let errors: Vec<Value> = report
        .errors
        .iter()
        .map(|e| json!({ "code": e.code, "attribute": e.attribute, "message": error_message(&e.code, &e.attribute) }))
        .collect();

    let groups: Vec<Value> = ["Basic", "Combined", "Credit Enhancement"]
        .iter()
        .map(|category| {
            let types: Vec<Value> = entries
                .iter()
                .filter(|e| e.category == *category)
                .map(sidebar_entry)
                .collect();
            json!({ "name": category, "types": types })
        })
        .collect();

    let engine_count = entries.iter().filter(|e| e.has_engine).count();

    json!({
        "title": "ACTUS Explorer",
        "tagline": "Algorithmic Contract Types",
        "versionLabel": format!("v1.1 \u{b7} {} Contract Types", entries.len()),
        "engineLabel": format!("{engine_count} engines registered"),
        "selectedAcronym": selected_entry.acronym,
        "search": "",
        "sidebarGroups": groups,
        "filteredCount": entries.len(),
        "chartView": "bar",
        "mainView": "timeline",
        "scenarioOpen": false,
        "scenario": {
            "rates": [],
            "events": [],
            "hasEntries": false,
        },
        "newRate": { "code": "ADR-MKT", "time": "2026-06-30", "value": "0.06" },
        "newObserved": { "eventType": "PP", "time": "2027-06-30", "payoff": "50000" },
        "selected": {
            "acronym": selected_entry.acronym,
            "name": selected_entry.name,
            "category": selected_entry.category,
            "description": selected_entry.description,
            "spec": selected_entry.spec,
            "hasEngine": selected_entry.has_engine,
            "keyEvents": selected_entry.key_events,
            "params": params,
            "paramCount": params.len(),
            "validation": {
                "valid": report.valid,
                "errors": errors,
                "errorCount": report.errors.len(),
            },
            "hasEvents": !events.is_empty(),
            "eventCount": events.len(),
            "events": event_views,
            "timeline": timeline_view(&events, min, range),
            "calendar": calendar_view(&events, parse_iso_period_days(values.get("gracePeriod"))),
            "stats": stats_view(&events),
            "chart": chart_view(&events),
            "chips": [],
            "hasChips": false,
            "engineNotice": engine_notice,
            "evaluationError": "",
            "hasEvaluationError": false,
            "isBasic": selected_entry.category == "Basic",
            "isCombined": selected_entry.category == "Combined",
            "isCredit": selected_entry.category == "Credit Enhancement",
        },
        "matrix": matrix_view(selected_type.as_acronym()),
    })
}

/// First/last event timestamps in epoch seconds and their span.
fn event_bounds(events: &[EventDto]) -> (i64, i64) {
    let times: Vec<i64> = events
        .iter()
        .filter_map(|e| {
            chrono::NaiveDateTime::parse_from_str(&e.event_date, dto::TIMESTAMP_FORMAT)
                .ok()
                .map(|t| t.and_utc().timestamp())
        })
        .collect();
    let min = times.iter().copied().min().unwrap_or(0);
    let max = times.iter().copied().max().unwrap_or(0);
    (min, max - min)
}

/// Splits an authored spec label `Notional (NT)` into `("Notional", "NT")`;
/// labels without a trailing parenthesized acronym keep an empty code.
fn split_label(label: &str) -> (String, String) {
    if let Some(open) = label.rfind(" (") {
        if label.ends_with(')') {
            let code = label[open + 2..label.len() - 1].to_string();
            if !code.is_empty()
                && code
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            {
                return (label[..open].to_string(), code);
            }
        }
    }
    (label.to_string(), String::new())
}

/// Human message of one validation error code.
fn error_message(code: &str, attribute: &str) -> String {
    match code {
        "AttributeNotApplicable" => format!("{attribute} is not applicable to this contract type"),
        "MissingAttribute" => format!("{attribute} is required but not set"),
        "UnknownAttribute" => format!("{attribute} is not in the ACTUS dictionary"),
        other => format!("{other}: {attribute}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_contains_seed_markers() {
        let state = build_state("PAM");
        assert_eq!(state["title"], "ACTUS Explorer");
        assert_eq!(state["selected"]["acronym"], "PAM");
        assert!(state["selected"]["hasEvents"].as_bool().unwrap());
        assert!(state["selected"]["events"].as_array().unwrap().len() > 3);
        assert!(state["selected"]["validation"]["valid"].as_bool().unwrap());
        assert!(!state["selected"]["chart"]["linePoints"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(state["matrix"]["rows"].as_array().unwrap().len(), 21);
        assert_eq!(state["matrix"]["attrCount"].as_u64().unwrap(), 124);
    }

    #[test]
    fn money_short_formatting() {
        assert_eq!(format_money_short(1_500_000.0, true), "+1.50M");
        assert_eq!(format_money_short(-2500.0, false), "\u{2212}2.50K");
        assert_eq!(format_money_short(0.0, true), "\u{2014}");
    }
}
