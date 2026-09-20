// Client-side view computation: transforms WASM evaluation results into the
// render state consumed by the app-shell template. Mirrors the server-side
// logic in crates/actus-web/src/state.rs (both feed the same template).

export interface OptionChoice {
  value: string;
  label: string;
}

export interface ParamSpec {
  key: string;
  label: string;
  kind: 'number' | 'date' | 'select' | 'text';
  value: string;
  step?: string | null;
  options?: OptionChoice[] | null;
}

export interface ValidationReport {
  valid: boolean;
  errors: { code: string; attribute: string }[];
  termStatus: Record<string, string>;
}

export interface EventDto {
  eventDate: string;
  eventType: string;
  payoff: string;
  currency: string | null;
  notionalPrincipal: string;
  nominalInterestRate: string;
  accruedInterest: string;
}

export interface EvaluationResult {
  events: EventDto[];
  contractStatus: string;
}

export interface EventView {
  dateText: string;
  eventType: string;
  eventLabel: string;
  payoffText: string;
  notionalText: string;
  leftPct: string;
  kind: string;
  side: string;
  sign: string;
  isLast: boolean;
}

export interface LaneEventView {
  dateText: string;
  eventType: string;
  eventLabel: string;
  payoffText: string;
  notionalText: string;
  leftPct: string;
  kind: string;
  sign: string;
}

export interface TimelineLane {
  key: string;
  label: string;
  hasLane: boolean;
  events: LaneEventView[];
}

export interface YearTick {
  leftPct: string;
  label: string;
}

export interface TimelineView {
  hasEvents: boolean;
  eventCount: number;
  axisMinWidth: string;
  yearTicks: YearTick[];
  lanes: TimelineLane[];
}

export interface ParamView {
  key: string;
  label: string;
  code: string;
  kind: string;
  value: string;
  step?: string | null;
  options?: OptionChoice[] | null;
  status: string;
  invalid: boolean;
  errorCode: string;
}

const EVENT_LABELS: Record<string, string> = {
  IED: 'Initial Exchange',
  IP: 'Interest Payment',
  PR: 'Principal Redemption',
  MD: 'Maturity',
  PP: 'Principal Payment',
  FP: 'Fee Payment',
  RR: 'Rate Reset',
  DV: 'Dividend',
  STD: 'Settlement',
  XD: 'Exercise',
  CE: 'Credit Event',
  IPCI: 'Interest Capitalization',
  PY: 'Penalty',
  PI: 'Principal Increase',
  AD: 'Analysis Date',
};

export function eventKind(eventType: string): string {
  switch (eventType) {
    case 'IED':
      return 'ied';
    case 'IP':
    case 'IPCI':
      return 'ip';
    case 'PR':
    case 'PP':
    case 'PY':
    case 'PI':
      return 'pr';
    case 'MD':
      return 'md';
    default:
      return 'other';
  }
}

export function formatMoneyShort(value: number, signed: boolean): string {
  const magnitude = Math.abs(value);
  let text: string;
  if (magnitude >= 1e9) text = (magnitude / 1e9).toFixed(2) + 'B';
  else if (magnitude >= 1e6) text = (magnitude / 1e6).toFixed(2) + 'M';
  else if (magnitude >= 1e3) text = (magnitude / 1e3).toFixed(2) + 'K';
  else if (magnitude === 0) return '\u2014';
  else text = magnitude.toFixed(2);
  const sign = value < 0 ? '\u2212' : signed ? '+' : '';
  return `${sign}${text}`;
}

function timestampMs(dateText: string): number {
  return Date.parse(dateText);
}

export function buildEventViews(events: EventDto[]): EventView[] {
  const times = events.map((e) => timestampMs(e.eventDate));
  const min = times.length ? Math.min(...times) : 0;
  const range = times.length ? Math.max(...times) - min : 0;
  return events.map((event, index) => {
    const payoff = Number(event.payoff) || 0;
    const time = timestampMs(event.eventDate);
    const ratio = range > 0 ? Math.min(Math.max(time - min, 0), range) / range : 0;
    const left = 4 + 92 * ratio;
    return {
      dateText: event.eventDate,
      eventType: event.eventType,
      eventLabel: EVENT_LABELS[event.eventType] ?? event.eventType,
      payoffText: formatMoneyShort(payoff, true),
      notionalText: formatMoneyShort(Number(event.notionalPrincipal) || 0, false),
      leftPct: left.toFixed(2),
      kind: eventKind(event.eventType),
      side: index % 2 === 0 ? 'top' : 'bottom',
      sign: payoff > 0 ? 'in' : payoff < 0 ? 'out' : 'zero',
      isLast: index + 1 === events.length,
    };
  });
}

/** Left-edge percentage of a time on the [min, min+range] axis (4%..96%). */
function axisLeftPct(time: number, min: number, range: number): string {
  const ratio = range > 0 ? Math.min(Math.max(time - min, 0), range) / range : 0;
  return (4 + 92 * ratio).toFixed(2);
}

const CALENDAR_MAX_MONTHS = 120;
const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

export interface CalendarDayCell {
  blank: boolean;
  day: number;
  hasEvent: boolean;
  grace: boolean;
  kind?: string;
  badges?: { type: string; kind: string }[];
  more?: number;
  lines?: { eventType: string; payoffText: string }[];
}

export interface CalendarMonth {
  key: string;
  label: string;
  hasEvents: boolean;
  cells: CalendarDayCell[];
}

export interface CalendarYear {
  year: number;
  label: string;
  months: CalendarMonth[];
}

export interface CalendarView {
  hasEvents: boolean;
  truncated: boolean;
  notice: string;
  years: CalendarYear[];
}

/** Approximate day count of an ISO period (`P10D`, `P2M`, `P1Y`), or null.
 * Mirrors `parse_iso_period_days` in state.rs. */
export function parseIsoPeriodDays(raw: string | undefined | null): number | null {
  if (!raw) return null;
  const rest = raw.trim().slice(1);
  if (!raw.trim().startsWith('P') || !rest) return null;
  const factors: Record<string, number> = { D: 1, W: 7, M: 30, Y: 365 };
  const re = /(\d+)([DWMY])/g;
  let total = 0;
  let matched = 0;
  for (const m of raw.matchAll(re)) {
    total += Number(m[1]) * factors[m[2]];
    matched += m[0].length;
  }
  return matched === raw.trim().length ? total : null;
}

/** Epoch ms of UTC midnight of the day in an `YYYY-MM-DD[THH:MM:SS]`
 * timestamp. Deliberately ignores the time-of-day and the browser timezone:
 * calendar day keys must match `Date.UTC(y, m, d)` exactly, or lookups miss
 * for any user not in UTC (Date.parse would apply the local offset). */
function utcDayKey(dateText: string): number {
  const [y, m, d] = dateText.slice(0, 10).split('-').map(Number);
  return Date.UTC(y, m - 1, d);
}

/** The calendar schedule view (mirrors `calendar_view` in state.rs). */
export function buildCalendar(events: EventDto[], graceDays: number | null): CalendarView {
  if (!events.length) {
    return { hasEvents: false, truncated: false, notice: '', years: [] };
  }
  const DAY_MS = 86_400_000;
  const byDate = new Map<number, EventDto[]>();
  for (const event of events) {
    const t = utcDayKey(event.eventDate);
    const list = byDate.get(t) ?? [];
    list.push(event);
    byDate.set(t, list);
  }
  const graceSet = new Set<number>();
  if (graceDays !== null && graceDays > 0) {
    for (const t of byDate.keys()) {
      for (let offset = 1; offset <= graceDays; offset += 1) {
        graceSet.add(t + offset * DAY_MS);
      }
    }
  }
  const times = [...byDate.keys()].sort((a, b) => a - b);
  const first = new Date(times[0]);
  const last = new Date(times[times.length - 1]);
  const years: CalendarYear[] = [];
  let rendered = 0;
  let truncated = false;
  let cursor = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth(), 1));
  const end = new Date(Date.UTC(last.getUTCFullYear(), last.getUTCMonth(), 1));
  let currentYear: number | null = null;
  let currentMonths: CalendarMonth[] = [];
  while (rendered < CALENDAR_MAX_MONTHS) {
    if (currentYear !== cursor.getUTCFullYear()) {
      if (currentYear !== null) {
        years.push({ year: currentYear, label: String(currentYear), months: currentMonths });
        currentMonths = [];
      }
      currentYear = cursor.getUTCFullYear();
    }
    currentMonths.push(monthView(cursor, byDate, graceSet));
    rendered += 1;
    if (cursor.getTime() >= end.getTime()) break;
    cursor = new Date(Date.UTC(cursor.getUTCFullYear(), cursor.getUTCMonth() + 1, 1));
  }
  if (currentYear !== null) {
    years.push({ year: currentYear, label: String(currentYear), months: currentMonths });
  }
  truncated = rendered >= CALENDAR_MAX_MONTHS && cursor.getTime() < end.getTime();
  const notice = truncated
    ? `Showing the first ${CALENDAR_MAX_MONTHS} months from ${MONTH_NAMES[first.getUTCMonth()]} ${first.getUTCFullYear()} — adjust the term dates to explore later periods.`
    : '';
  return { hasEvents: true, truncated, notice, years };
}

function monthView(
  monthStart: Date,
  byDate: Map<number, EventDto[]>,
  graceSet: Set<number>,
): CalendarMonth {
  const year = monthStart.getUTCFullYear();
  const month = monthStart.getUTCMonth();
  const offset = (monthStart.getUTCDay() + 6) % 7;
  const daysInMonth = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  const cells: CalendarDayCell[] = [];
  for (let i = 0; i < offset; i += 1) cells.push({ blank: true, day: 0, hasEvent: false, grace: false });
  for (let day = 1; day <= daysInMonth; day += 1) {
    const dayEvents = byDate.get(Date.UTC(year, month, day)) ?? [];
    const inGrace = dayEvents.length > 0 || graceSet.has(Date.UTC(year, month, day));
    if (!dayEvents.length) {
      cells.push({ blank: false, day, hasEvent: false, grace: inGrace });
      continue;
    }
    const badges = dayEvents.slice(0, 2).map((e) => ({
      type: e.eventType,
      kind: eventKind(e.eventType),
    }));
    const more = Math.max(dayEvents.length - 2, 0);
    cells.push({
      blank: false,
      day,
      hasEvent: true,
      grace: inGrace,
      kind: eventKind(dayEvents[0].eventType),
      badges,
      more,
      lines: dayEvents.map((e) => ({
        eventType: e.eventType,
        payoffText: formatMoneyShort(Number(e.payoff) || 0, true),
      })),
    });
  }
  return {
    key: `${year}-${String(month + 1).padStart(2, '0')}`,
    label: MONTH_NAMES[month],
    hasEvents: [...byDate.keys()].some((t) => {
      const d = new Date(t);
      return d.getUTCFullYear() === year && d.getUTCMonth() === month;
    }),
    cells,
  };
}

const LANES: { key: string; label: string }[] = [
  { key: 'ied', label: 'Initial exchange' },
  { key: 'ip', label: 'Interest' },
  { key: 'pr', label: 'Principal' },
  { key: 'md', label: 'Maturity' },
  { key: 'other', label: 'Other' },
];

export function buildTimeline(events: EventDto[]): TimelineView {
  const times = events.map((e) => timestampMs(e.eventDate));
  const min = times.length ? Math.min(...times) : 0;
  const range = times.length ? Math.max(...times) - min : 0;
  const laneEvents = new Map<string, LaneEventView[]>(LANES.map((l) => [l.key, []]));
  for (const event of events) {
    const payoff = Number(event.payoff) || 0;
    const kind = eventKind(event.eventType);
    laneEvents.get(kind)?.push({
      dateText: event.eventDate,
      eventType: event.eventType,
      eventLabel: EVENT_LABELS[event.eventType] ?? event.eventType,
      payoffText: formatMoneyShort(payoff, true),
      notionalText: formatMoneyShort(Number(event.notionalPrincipal) || 0, false),
      leftPct: axisLeftPct(timestampMs(event.eventDate), min, range),
      kind,
      sign: payoff > 0 ? 'in' : payoff < 0 ? 'out' : 'zero',
    });
  }
  const yearTicks: YearTick[] = [];
  if (events.length) {
    const y0 = new Date(min).getUTCFullYear() + 1;
    const y1 = new Date(min + range).getUTCFullYear();
    const span = Math.max(y1 - y0 + 1, 1);
    const step = Math.max(1, Math.ceil(span / 12));
    for (let year = y0; year <= y1; year += step) {
      const t = Date.UTC(year, 0, 1);
      if (t < min || t > min + range) continue;
      yearTicks.push({ leftPct: axisLeftPct(t, min, range), label: String(year) });
    }
  }
  const axisMinWidth = `${Math.min(Math.max(events.length * 30, 900), 6400)}px`;
  return {
    hasEvents: events.length > 0,
    eventCount: events.length,
    axisMinWidth,
    yearTicks,
    lanes: LANES.map((lane) => {
      const laneList = laneEvents.get(lane.key) ?? [];
      return { key: lane.key, label: lane.label, hasLane: laneList.length > 0, events: laneList };
    }),
  };
}

export function buildStats(events: EventDto[]): { label: string; value: string; tone: string }[] {
  let totalInterest = 0;
  let totalPrincipal = 0;
  for (const event of events) {
    const payoff = Math.abs(Number(event.payoff) || 0);
    const kind = eventKind(event.eventType);
    if (kind === 'ip') totalInterest += payoff;
    else if (kind === 'pr' || kind === 'md') totalPrincipal += payoff;
  }
  const finalNotional =
    events.length ? Number(events[events.length - 1].notionalPrincipal) || 0 : 0;
  return [
    { label: 'Total Interest', value: formatMoneyShort(totalInterest, false), tone: 'good' },
    { label: 'Total Principal', value: formatMoneyShort(totalPrincipal, false), tone: 'neutral' },
    { label: 'Events', value: String(events.length), tone: 'neutral' },
    {
      label: 'Final Notional',
      value: formatMoneyShort(finalNotional, false),
      tone: finalNotional >= 0 ? 'good' : 'bad',
    },
  ];
}

export interface ChartView {
  hasData: boolean;
  bars: { heightPct: string; sign: string; label: string }[];
  linePoints: string;
  cumulativePoints: string;
  yTicks: { bottomPct: string; label: string }[];
}

export function buildChart(events: EventDto[]): ChartView {
  if (!events.length) {
    return { hasData: false, bars: [], linePoints: '', cumulativePoints: '', yTicks: [] };
  }
  const payoffs = events.map((e) => Number(e.payoff) || 0);
  const maxAbs = Math.max(...payoffs.map((p) => Math.abs(p)), 1);
  const step = 1000 / Math.max(payoffs.length - 1, 1);
  let cumulative = 0;
  const cumulativeValues: number[] = [];
  const bars = payoffs.map((payoff) => {
    cumulative += payoff;
    cumulativeValues.push(cumulative);
    const height = Math.min(Math.max((Math.abs(payoff) / maxAbs) * 100, 2), 100);
    return {
      heightPct: height.toFixed(2),
      sign: payoff > 0 ? 'in' : payoff < 0 ? 'out' : 'zero',
      label: formatMoneyShort(payoff, true),
    };
  });
  const cumMax = Math.max(...cumulativeValues.map((v) => Math.abs(v)), 1);
  const points = (values: number[], bound: number): string =>
    values
      .map((v, i) => {
        const x = step * i;
        const y = 300 - 20 - 260 * ((Math.min(Math.max(v / bound, -1), 1) + 1) / 2);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' ');
  const yTicks = [1, 0.5, 0, -0.5, -1].map((share) => ({
    bottomPct: (((share + 1) / 2) * 100).toFixed(1),
    label: formatMoneyShort(maxAbs * share, true),
  }));
  return {
    hasData: true,
    bars,
    linePoints: points(payoffs, maxAbs),
    cumulativePoints: points(cumulativeValues, cumMax),
    yTicks,
  };
}

export function buildParamViews(specs: ParamSpec[], report: ValidationReport): ParamView[] {
  const errorByAttribute = new Map(report.errors.map((e) => [e.attribute, e.code]));
  return specs.map((spec) => {
    const fromStatus = report.termStatus[spec.key];
    const fromError = errorByAttribute.get(spec.key);
    const status = fromStatus ?? (fromError === 'MissingAttribute' ? 'required-missing' : fromError ? 'optional-set' : 'optional-unset');
    const labelMatch = spec.label.match(/\s*\(([A-Z0-9]+)\)$/);
    const label = labelMatch ? spec.label.slice(0, labelMatch.index) : spec.label;
    const code = labelMatch ? labelMatch[1] : '';
    return {
      key: spec.key,
      label,
      code,
      kind: spec.kind,
      value: spec.value,
      step: spec.step,
      options: spec.options,
      status,
      invalid: fromError !== undefined,
      errorCode: fromError ?? '',
    };
  });
}

export interface ErrorView {
  code: string;
  attribute: string;
  message: string;
}

export function buildErrors(report: ValidationReport): ErrorView[] {
  return report.errors.map((e) => ({
    code: e.code,
    attribute: e.attribute,
    message: errorMessage(e.code, e.attribute),
  }));
}

function errorMessage(code: string, attribute: string): string {
  switch (code) {
    case 'AttributeNotApplicable':
      return `${attribute} is not applicable to this contract type`;
    case 'MissingAttribute':
      return `${attribute} is required but not set`;
    case 'UnknownAttribute':
      return `${attribute} is not in the ACTUS dictionary`;
    default:
      return `${code}: ${attribute}`;
  }
}

export interface Chip {
  text: string;
  tone: 'add' | 'del';
}

export function diffEventTypes(previous: Set<string> | null, events: EventDto[]): Chip[] {
  const current = new Set(events.map((e) => e.eventType));
  if (!previous) return [];
  const chips: Chip[] = [];
  for (const type of current) {
    if (!previous.has(type)) chips.push({ text: `+${type} events`, tone: 'add' });
  }
  for (const type of previous) {
    if (!current.has(type)) chips.push({ text: `\u2212${type} events`, tone: 'del' });
  }
  return chips;
}
