// Unit tests for the client-side view computation (src/view.ts).
// `npm test` runs this file once per timezone in `package.json`, because
// every date computation must give the same answer in any browser timezone
// (and match the server-side render in state.rs, which works in UTC).

import { describe, expect, it } from 'vitest';

import {
  buildCalendar,
  buildChart,
  buildErrors,
  buildEventViews,
  buildParamViews,
  buildStats,
  buildTimeline,
  diffEventTypes,
  eventKind,
  formatMoneyShort,
  parseIsoPeriodDays,
  type EventDto,
  type ParamSpec,
  type ValidationReport,
} from '../src/view';

function event(eventDate: string, eventType: string, payoff: string, notional = '1000'): EventDto {
  return {
    eventDate,
    eventType,
    payoff,
    currency: 'USD',
    notionalPrincipal: notional,
    nominalInterestRate: '0.05',
    accruedInterest: '0',
  };
}

// A one-year PAM: IED, two semiannual IPs, MD.
const PAM: EventDto[] = [
  event('2013-01-01T00:00:00', 'IED', '-1000'),
  event('2013-07-01T00:00:00', 'IP', '24.79'),
  event('2014-01-01T00:00:00', 'IP', '25.21'),
  event('2014-01-01T00:00:00', 'MD', '1000', '0'),
];

describe('eventKind', () => {
  it('groups event types into the timeline lanes', () => {
    expect(eventKind('IED')).toBe('ied');
    expect(['IP', 'IPCI', 'IPFX', 'IPFL'].map(eventKind)).toEqual(['ip', 'ip', 'ip', 'ip']);
    expect(['PR', 'PP', 'PY', 'PI'].map(eventKind)).toEqual(['pr', 'pr', 'pr', 'pr']);
    expect(eventKind('MD')).toBe('md');
    expect(['RR', 'FP', 'XYZ', ''].map(eventKind)).toEqual(['other', 'other', 'other', 'other']);
  });
});

describe('formatMoneyShort', () => {
  it('abbreviates by magnitude with two decimals', () => {
    expect(formatMoneyShort(12.345, false)).toBe('12.35');
    expect(formatMoneyShort(1500, false)).toBe('1.50K');
    expect(formatMoneyShort(2_500_000, false)).toBe('2.50M');
    expect(formatMoneyShort(7_000_000_000, false)).toBe('7.00B');
  });

  it('signs negatives always and positives only when asked', () => {
    expect(formatMoneyShort(-1500, false)).toBe('−1.50K');
    expect(formatMoneyShort(1500, true)).toBe('+1.50K');
    expect(formatMoneyShort(1500, false)).toBe('1.50K');
  });

  it('renders zero as an em dash regardless of sign flag', () => {
    expect(formatMoneyShort(0, true)).toBe('—');
    expect(formatMoneyShort(-0, false)).toBe('—');
  });
});

describe('parseIsoPeriodDays', () => {
  it('approximates days of ISO periods', () => {
    expect(parseIsoPeriodDays('P10D')).toBe(10);
    expect(parseIsoPeriodDays('P2W')).toBe(14);
    expect(parseIsoPeriodDays('P2M')).toBe(60);
    expect(parseIsoPeriodDays('P1Y')).toBe(365);
    expect(parseIsoPeriodDays('P1Y2M3D')).toBe(365 + 60 + 3);
    expect(parseIsoPeriodDays(' P5D ')).toBe(5);
  });

  it('rejects empty, unprefixed and malformed periods', () => {
    for (const raw of [undefined, null, '', 'P', '10D', 'P10', 'P10X', 'P1DX', 'PT1H']) {
      expect(parseIsoPeriodDays(raw)).toBeNull();
    }
  });
});

describe('buildEventViews', () => {
  it('positions events on a 4%..96% axis and labels them', () => {
    const views = buildEventViews(PAM);
    expect(views.map((v) => v.leftPct)).toEqual(['4.00', '49.62', '96.00', '96.00']);
    expect(views[0]).toMatchObject({
      eventLabel: 'Initial Exchange',
      payoffText: '−1.00K',
      notionalText: '1.00K',
      kind: 'ied',
      side: 'top',
      sign: 'out',
      isLast: false,
    });
    expect(views[1]).toMatchObject({ side: 'bottom', sign: 'in', payoffText: '+24.79' });
    expect(views[3]).toMatchObject({ eventLabel: 'Maturity', notionalText: '—', isLast: true });
  });

  it('handles a single event, unknown types and unparseable payoffs', () => {
    const [only] = buildEventViews([event('2013-01-01T00:00:00', 'XYZ', 'n/a')]);
    expect(only).toMatchObject({ leftPct: '4.00', eventLabel: 'XYZ', sign: 'zero', isLast: true });
    expect(buildEventViews([])).toEqual([]);
  });
});

describe('buildTimeline', () => {
  it('puts each event in its lane and keeps empty lanes flagged', () => {
    const timeline = buildTimeline(PAM);
    expect(timeline.hasEvents).toBe(true);
    expect(timeline.eventCount).toBe(4);
    const lanes = Object.fromEntries(timeline.lanes.map((l) => [l.key, l]));
    expect(lanes.ied.events).toHaveLength(1);
    expect(lanes.ip.events.map((e) => e.leftPct)).toEqual(['49.62', '96.00']);
    expect(lanes.md.events[0].leftPct).toBe('96.00');
    expect(lanes.pr.hasLane).toBe(false);
    expect(lanes.other.hasLane).toBe(false);
  });

  it('places a year tick exactly on a 1 January event', () => {
    // The MD is at 2014-01-01, so the 2014 tick must sit on it.
    expect(buildTimeline(PAM).yearTicks).toEqual([{ leftPct: '96.00', label: '2014' }]);
  });

  it('thins year ticks on long schedules', () => {
    const long = [event('2000-06-01T00:00:00', 'IED', '-1'), event('2040-06-01T00:00:00', 'MD', '1')];
    const ticks = buildTimeline(long).yearTicks;
    expect(ticks.length).toBeLessThanOrEqual(14);
    expect(ticks[0].label).toBe('2001');
    expect(Number(ticks[1].label) - Number(ticks[0].label)).toBe(4);
  });

  it('clamps the axis width', () => {
    expect(buildTimeline([]).axisMinWidth).toBe('900px');
    expect(buildTimeline([]).hasEvents).toBe(false);
    const many = Array.from({ length: 400 }, (_, i) =>
      event(`20${String(10 + Math.floor(i / 12)).padStart(2, '0')}-${String((i % 12) + 1).padStart(2, '0')}-01T00:00:00`, 'IP', '1'),
    );
    expect(buildTimeline(many).axisMinWidth).toBe('6400px');
  });
});

describe('buildStats', () => {
  it('sums interest and principal magnitudes and reports the final notional', () => {
    expect(buildStats(PAM)).toEqual([
      { label: 'Total Interest', value: '50.00', tone: 'good' },
      { label: 'Total Principal', value: '1.00K', tone: 'neutral' },
      { label: 'Events', value: '4', tone: 'neutral' },
      { label: 'Final Notional', value: '—', tone: 'good' },
    ]);
  });

  it('flags a negative final notional', () => {
    const stats = buildStats([event('2013-01-01T00:00:00', 'IED', '1000', '-1000')]);
    expect(stats[3]).toEqual({ label: 'Final Notional', value: '−1.00K', tone: 'bad' });
    expect(buildStats([])[2].value).toBe('0');
  });
});

describe('buildChart', () => {
  it('scales bars to the largest payoff with a 2% floor', () => {
    const chart = buildChart(PAM);
    expect(chart.hasData).toBe(true);
    expect(chart.bars.map((b) => b.heightPct)).toEqual(['100.00', '2.48', '2.52', '100.00']);
    expect(chart.bars.map((b) => b.sign)).toEqual(['out', 'in', 'in', 'in']);
    expect(chart.yTicks.map((t) => t.bottomPct)).toEqual(['100.0', '75.0', '50.0', '25.0', '0.0']);
    expect(chart.yTicks[0].label).toBe('+1.00K');
  });

  it('draws line and cumulative polylines across the 1000-wide plot', () => {
    const chart = buildChart(PAM);
    const xs = (points: string) => points.split(' ').map((p) => p.split(',')[0]);
    expect(xs(chart.linePoints)).toEqual(['0.0', '333.3', '666.7', '1000.0']);
    // Cumulative ends at +50 (interest earned); the first point is the -1000 low.
    const ys = chart.cumulativePoints.split(' ').map((p) => Number(p.split(',')[1]));
    expect(ys[0]).toBe(280);
    expect(ys[3]).toBeLessThan(ys[0]);
  });

  it('is empty without events', () => {
    expect(buildChart([])).toEqual({ hasData: false, bars: [], linePoints: '', cumulativePoints: '', yTicks: [] });
  });
});

describe('buildCalendar', () => {
  it('renders every month between the first and last event', () => {
    const calendar = buildCalendar(PAM, null);
    expect(calendar.hasEvents).toBe(true);
    expect(calendar.truncated).toBe(false);
    expect(calendar.years.map((y) => [y.year, y.months.length])).toEqual([
      [2013, 12],
      [2014, 1],
    ]);
  });

  it('marks event days in the browser-independent UTC day', () => {
    const january = buildCalendar(PAM, null).years[0].months[0];
    expect(january).toMatchObject({ key: '2013-01', label: 'Jan', hasEvents: true });
    // 2013-01-01 is a Tuesday: one blank Monday cell first.
    expect(january.cells[0].blank).toBe(true);
    const first = january.cells[1];
    expect(first).toMatchObject({ day: 1, hasEvent: true, kind: 'ied', more: 0 });
    expect(first.lines).toEqual([{ eventType: 'IED', payoffText: '−1.00K' }]);
    expect(january.cells.filter((c) => c.hasEvent)).toHaveLength(1);
    expect(buildCalendar(PAM, null).years[0].months[1].hasEvents).toBe(false);
  });

  it('caps badges at two and counts the rest', () => {
    const busy = ['IP', 'PR', 'RR', 'FP'].map((t) => event('2013-03-15T00:00:00', t, '1'));
    const day = buildCalendar(busy, null).years[0].months[0].cells.find((c) => c.day === 15)!;
    expect(day.badges).toEqual([
      { type: 'IP', kind: 'ip' },
      { type: 'PR', kind: 'pr' },
    ]);
    expect(day.more).toBe(2);
    expect(day.lines).toHaveLength(4);
  });

  it('shades the grace period after each event', () => {
    const march = buildCalendar([event('2013-03-10T00:00:00', 'IP', '1')], 3).years[0].months[0];
    const grace = march.cells.filter((c) => !c.blank && c.grace).map((c) => c.day);
    expect(grace).toEqual([10, 11, 12, 13]);
    const none = buildCalendar([event('2013-03-10T00:00:00', 'IP', '1')], 0).years[0].months[0];
    expect(none.cells.filter((c) => !c.blank && c.grace).map((c) => c.day)).toEqual([10]);
  });

  it('truncates very long schedules at 120 months with a notice', () => {
    const long = [event('2000-01-01T00:00:00', 'IED', '-1'), event('2030-01-01T00:00:00', 'MD', '1')];
    const calendar = buildCalendar(long, null);
    expect(calendar.truncated).toBe(true);
    expect(calendar.years.flatMap((y) => y.months)).toHaveLength(120);
    expect(calendar.notice).toContain('first 120 months from Jan 2000');
  });

  it('is empty without events', () => {
    expect(buildCalendar([], 5)).toEqual({ hasEvents: false, truncated: false, notice: '', years: [] });
  });
});

describe('buildParamViews and buildErrors', () => {
  const specs: ParamSpec[] = [
    { key: 'notionalPrincipal', label: 'Notional Principal (NT)', kind: 'number', value: '' },
    { key: 'currency', label: 'Currency', kind: 'text', value: 'USD' },
    { key: 'feeRate', label: 'Fee Rate (FER)', kind: 'number', value: '0.01' },
    { key: 'calendar', label: 'Calendar (CLDR)', kind: 'select', value: '' },
  ];
  const report: ValidationReport = {
    valid: false,
    errors: [
      { code: 'MissingAttribute', attribute: 'notionalPrincipal' },
      { code: 'AttributeNotApplicable', attribute: 'feeRate' },
    ],
    termStatus: { currency: 'required-set' },
  };

  it('splits the acronym out of the label and derives each status', () => {
    const views = buildParamViews(specs, report);
    expect(views.map((v) => [v.label, v.code, v.status, v.invalid, v.errorCode])).toEqual([
      ['Notional Principal', 'NT', 'required-missing', true, 'MissingAttribute'],
      ['Currency', '', 'required-set', false, ''],
      ['Fee Rate', 'FER', 'optional-set', true, 'AttributeNotApplicable'],
      ['Calendar', 'CLDR', 'optional-unset', false, ''],
    ]);
  });

  it('explains every error code', () => {
    const errors = buildErrors({
      ...report,
      errors: [
        ...report.errors,
        { code: 'UnknownAttribute', attribute: 'foo' },
        { code: 'Other', attribute: 'bar' },
      ],
    });
    expect(errors.map((e) => e.message)).toEqual([
      'notionalPrincipal is required but not set',
      'feeRate is not applicable to this contract type',
      'foo is not in the ACTUS dictionary',
      'Other: bar',
    ]);
  });
});

describe('diffEventTypes', () => {
  it('reports added and removed event types between evaluations', () => {
    expect(diffEventTypes(null, PAM)).toEqual([]);
    expect(diffEventTypes(new Set(['IED', 'IP', 'MD', 'RR']), [...PAM, event('2013-04-01T00:00:00', 'PR', '1')])).toEqual([
      { text: '+PR events', tone: 'add' },
      { text: '−RR events', tone: 'del' },
    ]);
    expect(diffEventTypes(new Set(['IED', 'IP', 'MD']), PAM)).toEqual([]);
  });
});
