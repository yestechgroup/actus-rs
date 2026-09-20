// ACTUS Explorer root component: contract selection, live parameter
// editing with applicability validation, schedule evaluation through the
// actus-wasm bindings, and the risk-factor scenario editor.
import { WebUIElement, observable } from '@microsoft/webui-framework';
import type {
  CalendarView,
  ChartView,
  Chip,
  ErrorView,
  EventView,
  EvaluationResult,
  ParamSpec,
  ParamView,
  TimelineView,
  ValidationReport,
} from '../view.js';
import {
  buildCalendar,
  buildChart,
  buildErrors,
  buildEventViews,
  buildParamViews,
  buildStats,
  buildTimeline,
  diffEventTypes,
  parseIsoPeriodDays,
} from '../view.js';

interface SidebarType {
  acronym: string;
  name: string;
  description: string;
  hasEngine: boolean;
}

interface SidebarGroup {
  name: string;
  types: SidebarType[];
}

interface RateObservation {
  code: string;
  time: string;
  value: string;
  index: number;
}

interface ObservedEventInput {
  eventType: string;
  time: string;
  payoff: string;
  index: number;
}

interface SelectedState {
  acronym: string;
  name: string;
  category: string;
  description: string;
  spec: string;
  hasEngine: boolean;
  keyEvents: string[];
  params: ParamView[];
  paramCount: number;
  validation: { valid: boolean; errors: ErrorView[]; errorCount: number };
  hasEvents: boolean;
  eventCount: number;
  events: EventView[];
  timeline: TimelineView;
  calendar: CalendarView;
  stats: { cards: { label: string; value: string; tone: string }[]; hasCards: boolean };
  chart: ChartView;
  chips: Chip[];
  hasChips: boolean;
  engineNotice: string;
  evaluationError: string;
  hasEvaluationError: boolean;
}

interface ScenarioState {
  rates: RateObservation[];
  events: ObservedEventInput[];
  hasEntries: boolean;
}

// Shape of the wasm-pack generated bindings served from /pkg (built with
// `wasm-pack build crates/actus-wasm --target web`).
interface ActusModule {
  contract_types(): string;
  applicability(contractType: string): string;
  applicability_matrix(): string;
  attribute_meta(): string;
  contract_param_specs(contractType: string): string;
  build_terms_json(contractType: string, valuesJson: string): string;
  validate(termsJson: string): string;
  evaluate(termsJson: string): string;
  evaluate_with_risk(termsJson: string, scenarioJson: string): string;
  default(
    input?: { module_or_path: string } | Promise<ArrayBuffer>,
  ): Promise<{ memory: WebAssembly.Memory }>;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function prettyJson(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

export class AppShell extends WebUIElement {
  @observable selectedAcronym = 'PAM';  @observable sidebarGroups: SidebarGroup[] = [];
  @observable filteredCount = 0;
  @observable search = '';
  @observable selected!: SelectedState;
  @observable chartView = 'bar';
  @observable mainView = 'timeline';
  @observable scenario: ScenarioState = { rates: [], events: [], hasEntries: false };
  @observable wasmReady = false;
  @observable debugTerms = '';
  @observable debugResponse = '';
  @observable debugScenario = '';
  @observable debugValid = '';

  private actus: ActusModule | null = null;
  private values: Record<string, string> = {};
  private paramSpecs: ParamSpec[] = [];
  private previousEventTypes: Set<string> | null = null;
  private rateCounter = 0;
  private observedCounter = 0;

  private rateCodeEl!: HTMLInputElement;
  private rateTimeEl!: HTMLInputElement;
  private rateValueEl!: HTMLInputElement;
  private observedTypeEl!: HTMLSelectElement;
  private observedTimeEl!: HTMLInputElement;
  private observedPayoffEl!: HTMLInputElement;

  protected override hydratedCallback(): void {
    void this.bootstrap();
  }

  private async bootstrap(): Promise<void> {
    if (this.actus) return;
    try {
      const modulePath = '/pkg/actus_wasm.js';
      const actus = (await import(modulePath)) as ActusModule;
      await actus.default({ module_or_path: '/pkg/actus_wasm_bg.wasm' });
      this.actus = actus;
      this.wasmReady = true;
      // Recompute the SSR-rendered contract with the same client code path
      // and populate the JSON debug panel for the initially selected type.
      this.refresh(true, true);
    } catch (error) {
      console.error('actus-wasm failed to load', error);
    }
  }

  private parseReport(raw: string): ValidationReport {
    try {
      return JSON.parse(raw) as ValidationReport;
    } catch {
      return { valid: true, errors: [], termStatus: {} };
    }
  }

  private buildTerms(acronym: string): string {
    if (!this.actus) return '{}';
    try {
      return this.actus.build_terms_json(acronym, JSON.stringify(this.values));
    } catch (error) {
      console.error('terms build failed', error);
      return '{}';
    }
  }

  private evaluateTerms(
    acronym: string,
    terms: string,
  ): { result: EvaluationResult | null; error: string } {
    if (!this.actus) return { result: null, error: 'WASM bindings not loaded yet.' };
    const raw = this.scenarioJson();
    try {
      const result =
        raw === null
          ? this.actus.evaluate(terms)
          : this.actus.evaluate_with_risk(terms, raw);
      return { result: JSON.parse(result) as EvaluationResult, error: '' };
    } catch (error) {
      // Engine rejection (unsupported type, missing attribute, unobserved
      // risk factor): surface the message instead of failing silently.
      return { result: null, error: errorText(error) };
    }
  }

  private scenarioJson(): string | null {
    const { rates, events } = this.scenario;
    if (!rates.length && !events.length) return null;
    const scenario: Record<string, unknown> = {};
    if (rates.length) {
      const series: Record<string, Record<string, number>> = {};
      for (const rate of rates) {
        const points = series[rate.code] ?? {};
        points[`${rate.time}T00:00:00`] = Number(rate.value);
        series[rate.code] = points;
      }
      scenario.rates = series;
    }
    if (events.length) {
      scenario.observedEvents = events.map((event) => ({
        eventType: event.eventType,
        time: `${event.time}T00:00:00`,
        payoff: event.payoff,
      }));
    }
    return JSON.stringify(scenario);
  }

  private emptySelected(acronym: string, previous?: SelectedState): SelectedState {
    return {
      acronym,
      name: previous?.name ?? acronym,
      category: previous?.category ?? 'Basic',
      description: previous?.description ?? '',
      spec: previous?.spec ?? '',
      hasEngine: previous?.hasEngine ?? true,
      keyEvents: previous?.keyEvents ?? [],
      params: [],
      paramCount: 0,
      validation: { valid: true, errors: [], errorCount: 0 },
      hasEvents: false,
      eventCount: 0,
      events: [],
      timeline: buildTimeline([]),
      calendar: buildCalendar([], null),
      stats: { cards: [], hasCards: false },
      chart: { hasData: false, bars: [], linePoints: '', cumulativePoints: '', yTicks: [] },
      chips: [],
      hasChips: false,
      engineNotice:
        previous && !previous.hasEngine
          ? 'Engine not yet available for this contract type \u2014 showing the parameter form only.'
          : '',
      evaluationError: '',
      hasEvaluationError: false,
    };
  }

  private refresh(chipsEnabled: boolean, reseedValues = false): void {
    const acronym = this.selectedAcronym;
    if (!this.actus) return;
    let specs: ParamSpec[] = this.paramSpecs;
    if (chipsEnabled) {
      try {
        specs = JSON.parse(this.actus.contract_param_specs(acronym)) as ParamSpec[];
        this.paramSpecs = specs;
        if (reseedValues) {
          // A freshly selected contract starts from its spec defaults;
          // previously edited values belong to the prior contract.
          this.values = Object.fromEntries(specs.map((s) => [s.key, s.value]));
        }
      } catch (error) {
        console.error('param specs failed', error);
      }
    }
    const terms = this.buildTerms(acronym);
    const reportRaw = this.actus.validate(terms);
    const report = this.parseReport(reportRaw);
    const { result: evaluation, error: evaluationError } = this.evaluateTerms(acronym, terms);
    const events = evaluation?.events ?? [];
    const chips = chipsEnabled ? diffEventTypes(this.previousEventTypes, events) : [];
    this.previousEventTypes = new Set(events.map((e) => e.eventType));

    const previous = this.selected;
    const params = buildParamViews(specs, report);
    const view = this.emptySelected(acronym, previous);
    view.params = params;
    view.paramCount = params.length;
    view.validation = {
      valid: report.valid,
      errors: buildErrors(report),
      errorCount: report.errors.length,
    };
    view.hasEvents = events.length > 0;
    view.eventCount = events.length;
    view.events = buildEventViews(events);
    view.timeline = buildTimeline(events);
    view.calendar = buildCalendar(events, parseIsoPeriodDays(this.values['gracePeriod']));
    view.stats = { cards: buildStats(events), hasCards: events.length > 0 };
    view.chart = buildChart(events);
    view.chips = chips;
    view.hasChips = chips.length > 0;
    view.evaluationError = evaluationError;
    view.hasEvaluationError = evaluationError !== '';
    if (!view.hasEngine) {
      view.engineNotice =
        'Engine not yet available for this contract type \u2014 showing the parameter form only.';
    }
    this.selected = view;

    // Debug panel state: exactly what was sent and what came back.
    const scenario = this.scenarioJson();
    this.debugTerms = prettyJson(terms);
    this.debugValid = prettyJson(reportRaw);
    this.debugResponse = evaluation
      ? prettyJson(JSON.stringify(evaluation))
      : `(evaluation failed) ${evaluationError}`;
    this.debugScenario = scenario === null ? '(no scenario — evaluate() without risk factors)' : prettyJson(scenario);
  }

  copyDebug(e: Event): void {
    const button = e.target as HTMLButtonElement;
    const text = [
      `# contract: ${this.selectedAcronym}`,
      `# terms\n${this.debugTerms}`,
      `# validation\n${this.debugValid}`,
      `# scenario\n${this.debugScenario}`,
      `# response\n${this.debugResponse}`,
    ].join('\n\n');
    void navigator.clipboard.writeText(text).then(
      () => {
        button.textContent = 'Copied!';
        window.setTimeout(() => {
          button.textContent = 'Copy all';
        }, 1500);
      },
      () => {
        button.textContent = 'Copy failed';
      },
    );
  }

  selectContract(acronym: string): void {
    if (acronym === this.selectedAcronym) return;
    this.selectedAcronym = acronym;
    this.previousEventTypes = null;
    const entry = this.sidebarGroups
      .flatMap((group) => group.types)
      .find((type) => type.acronym === acronym);
    const previous = this.emptySelected(acronym, this.selected);
    previous.name = entry?.name ?? acronym;
    previous.hasEngine = entry?.hasEngine ?? true;
    previous.description = entry?.description ?? '';
    previous.spec = entry?.description ?? '';
    this.selected = previous;
    this.refresh(true, true);
  }

  onSearch(e: Event): void {
    const query = (e.target as HTMLInputElement).value.toLowerCase();
    this.search = query;
    this.applyFilter(query);
  }

  private applyFilter(query: string): void {
    this.sidebarGroups = this.sidebarGroups.map((group) => ({
      name: group.name,
      types: group.types.filter(
        (type) =>
          !query ||
          type.acronym.toLowerCase().includes(query) ||
          type.name.toLowerCase().includes(query),
      ),
    }));
    this.filteredCount = this.sidebarGroups.reduce((sum, group) => sum + group.types.length, 0);
  }

  onParamChange(key: string, e: Event): void {
    this.setParamValue(key, (e.target as HTMLInputElement).value);
  }

  /** Writes one param value into the form state and re-renders. */
  private setParamValue(key: string, value: string): void {
    this.values[key] = value;
    // Keep the spec copy in sync so re-rendered controls show the new value.
    this.paramSpecs = this.paramSpecs.map((spec) =>
      spec.key === key ? { ...spec, value } : spec,
    );
    this.refresh(false);
  }

  onParamSelect(key: string, value: string): void {
    this.setParamValue(key, value);
  }

  setMainView(view: string): void {
    this.mainView = view;
  }

  generate(): void {
    this.refresh(true);
  }
  setView(view: string): void {
    this.chartView = view;
  }

  addRate(): void {
    const code = this.rateCodeEl.value.trim();
    const time = this.rateTimeEl.value;
    const value = this.rateValueEl.value;
    if (!code || !time || !value) return;
    this.scenario = {
      ...this.scenario,
      rates: [...this.scenario.rates, { code, time, value, index: this.rateCounter++ }],
      hasEntries: true,
    };
    this.refresh(false);
  }

  removeRate(index: number): void {
    const rates = this.scenario.rates.filter((rate) => rate.index !== index);
    this.scenario = { ...this.scenario, rates, hasEntries: rates.length > 0 || this.scenario.events.length > 0 };
    this.refresh(false);
  }

  addObservedEvent(): void {
    const eventType = this.observedTypeEl.value;
    const time = this.observedTimeEl.value;
    const payoff = this.observedPayoffEl.value;
    if (!eventType || !time) return;
    this.scenario = {
      ...this.scenario,
      events: [
        ...this.scenario.events,
        { eventType, time, payoff, index: this.observedCounter++ },
      ],
      hasEntries: true,
    };
    this.refresh(false);
  }

  removeObservedEvent(index: number): void {
    const events = this.scenario.events.filter((event) => event.index !== index);
    this.scenario = { ...this.scenario, events, hasEntries: events.length > 0 || this.scenario.rates.length > 0 };
    this.refresh(false);
  }

  clearScenario(): void {
    this.scenario = { rates: [], events: [], hasEntries: false };
    this.refresh(false);
  }
}

AppShell.define('app-shell');
