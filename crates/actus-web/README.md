# ACTUS Explorer (`actus-web`)

Web visualizer for the ACTUS financial contract standard (issue #1), built
with Microsoft's [WebUI framework](https://microsoft.github.io/webui/) —
SSR templates compiled to a binary protocol, hydrated by a small
TypeScript client that evaluates contracts locally through the
`actus-wasm` WASM bindings. Plain CSS custom properties and template-driven
SVG/div charts; no CSS frameworks or JS chart libraries.

## Features

- **Contract explorer**: all 21 dictionary contract types in a searchable
  sidebar; the 8 engine-backed types (PAM, LAM, NAM, ANN, CSH, CLM, SWAPS,
  CEC) evaluate schedules; the rest show a generic applicability-derived
  form and degrade with an "engine not yet available" notice.
- **Applicability matrix**: 21 × 124 heatmap (`required` / base-required /
  applicable / not-applicable) with dictionary tooltips on the column
  headers; click a row to open that contract type.
- **Live validation**: every edit runs the WASM `validate()` — inline
  error badges (`AttributeNotApplicable`, `MissingAttribute`,
  `UnknownAttribute`), highlighted required-missing fields, and a disabled
  *Generate Schedule* button with a reason list until the terms are valid.
- **Terms → events demonstration**: re-evaluating after toggling an
  optional term annotates which event types appeared or disappeared
  (`+RR events`, `−IP events` chips next to the timeline).
- **Schedule visualization**: event timeline with color-coded markers
  (IED blue, IP green, PR amber, MD red, other purple) and hover tooltips,
  cash-flow chart (bar / line / cumulative tabs, div/SVG based), event
  table and four stat cards (total interest, total principal, event count,
  final notional).
- **Risk factor scenario** (`evaluateWithRisk`): rate observations keyed
  by market object code (drive `RR` resets via `RRMO`), and externally
  observed events (e.g. `PP` prepayments). Series are step functions: the
  latest observation at or before the requested time applies. Try: set
  `RRMO` + `RRCL`/`RRANX` on a PAM, then add rate observations in the
  scenario panel and watch the `RR` events and IP amounts change.

## Build & run (in order)

Prerequisites: Rust (2021), `wasm-pack`, Node.js ≥ 18 + npm.

```sh
# 1. Frontend dependencies (esbuild + @microsoft/webui-framework + typescript)
cd crates/actus-web/assets
npm install
cd ../../..

# 2. WASM bindings package served at /pkg (glue + .wasm)
wasm-pack build crates/actus-wasm --target web --out-dir pkg

# 3. Site build: compiles templates/CSS to dist/ (protocol.bin + app-shell.css)
#    via the microsoft-webui Rust build API, then bundles src/index.ts into
#    dist/index.js with esbuild.
cargo run -p actus-web --bin build-site

# 4. Serve (default 127.0.0.1:8080, override with $ACTUS_WEB_ADDR;
#    protocol/dist path override: $ACTUS_WEB_DIST)
cargo run -p actus-web --bin actus-web
# → http://127.0.0.1:8080
```

Useful checks:

```sh
cargo test -p actus-web          # SSR state + rendered-document integration tests
npm run typecheck --prefix crates/actus-web/assets   # tsc --noEmit over the client TS
```

The integration tests build the WebUI protocol in-process from
`assets/src` (pure Rust, no node needed), so they pass without step 1/2.

## Layout

```
crates/actus-web/
├── assets/                  # frontend npm project
│   ├── package.json         # @microsoft/webui-framework, esbuild, typescript
│   └── src/
│       ├── index.html       # entry template (loads /index.js)
│       ├── index.ts         # hydration entry point
│       ├── view.ts          # client-side view computation (mirrors state.rs)
│       └── app-shell/       # root interactive component (.html/.css/.ts)
├── src/
│   ├── lib.rs               # axum app: SSR handler + static /pkg serving
│   ├── state.rs             # SSR state builder (specs, matrix, event/chart views)
│   ├── main.rs              # server binary
│   └── bin/build-site.rs    # protocol build (webui::build) + esbuild bundling
└── tests/render.rs          # router-level SSR integration tests
```

## How SSR + hydration work here

1. `build-site` runs `webui::build` (plugin `Plugin::WebUI`, Light DOM,
   Link CSS) over `assets/src`, writing `dist/protocol.bin` and the
   component CSS; esbuild bundles `src/index.ts` → `dist/index.js` with
   `/pkg/*` kept external.
2. The axum server loads `dist/protocol.bin` via
   `webui::Protocol::from_protobuf` and renders every non-asset path with
   `WebUIHandler::with_plugin(WebUIHydrationPlugin)` + full SSR state
   (contract catalogue, applicability matrix, and the PAM default
   schedule precomputed in `state::build_state` through the same
   `actus-wasm` code paths the browser uses).
3. The browser hydrates `app-shell`; afterwards every interaction
   (selection, edits, scenario entries) calls the WASM functions locally
   (`contract_param_specs`, `build_terms_json`, `validate`,
   `evaluate`, `evaluate_with_risk`) and re-renders via template bindings
   (`?data-*` attribute bindings + CSS selectors).
