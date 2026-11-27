#!/usr/bin/env bash
# Creates the ROADMAP.md milestones on GitHub and assigns the existing
# issues to them. Idempotent: existing milestones (matched by title) are
# reused. Requires `gh` authenticated with write access to the repo.
set -euo pipefail

REPO="${REPO:-yestechgroup/actus-rs}"

milestone() { # title description
  local title="$1" desc="$2" num
  num=$(gh api "repos/$REPO/milestones?state=all&per_page=100" \
        --jq ".[] | select(.title == \"$title\") | .number")
  if [[ -z "$num" ]]; then
    num=$(gh api "repos/$REPO/milestones" -f title="$title" \
          -f description="$desc" --jq .number)
    echo "created  #$num $title" >&2
  else
    echo "exists   #$num $title" >&2
  fi
  echo "$num"
}

assign() { # milestone-number issue...
  local m="$1"; shift
  for issue in "$@"; do
    gh api -X PATCH "repos/$REPO/issues/$issue" -F milestone="$m" >/dev/null
    echo "  issue #$issue -> milestone #$m" >&2
  done
}

M02=$(milestone "v0.2 — Foundations" \
  "Repo hygiene, default engine registry, crate lints, CI coverage floor / cargo-deny / MSRV / codegen drift check, doc drift, coverage holes. See ROADMAP.md.")
M03=$(milestone "v0.3 — Spec completeness" \
  "Fix known spec gaps (#2-#8), BCS/CDSWP/MAR, close corpus exclusions (SC, fees, PP/PY, PRD/TD, progressed contracts, day counts), no-panic error policy. See ROADMAP.md.")
M04=$(milestone "v0.4 — Verification depth" \
  "Independent oracle for every engine, Java actus-core differential vectors, per-type property tests, cargo-fuzz, cargo-mutants, 95% coverage. See ROADMAP.md.")
M05=$(milestone "v0.5 — Formal verification" \
  "Kani proofs (day count, schedule, ordering, no-panic), checked arithmetic, generated spec traceability matrix. See ROADMAP.md.")
M06=$(milestone "v0.6 — Stable API & packaging" \
  "API review, JSON schemas, semver-checks, crates.io/npm pre-releases, docs.rs examples, benchmarks. See ROADMAP.md.")
M08=$(milestone "v0.8 — ACTUS Explorer" \
  "Per-family visualizations, state inspector, testbed preset gallery, permalinks, a11y/dark mode, GitHub Pages deploy, Playwright + visual regression. See ROADMAP.md.")
M10=$(milestone "v1.0 — Launch" \
  "README/brand, mdBook docs site, published conformance + verification report, release automation, crates.io/npm 1.0. See ROADMAP.md.")

assign "$M03" 2 3 4 5 6 7 8
assign "$M08" 1

echo "Milestones: $M02 $M03 $M04 $M05 $M06 $M08 $M10" >&2
