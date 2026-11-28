# Security policy

## Supported versions

actus-rs is pre-1.0 and has not published a release. Security fixes go into
`main` only.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it
privately through GitHub:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.

This opens a private advisory that only the maintainers can see. Please
include the affected crate and commit, a minimal reproduction (for example the
terms JSON) and the impact you observed.

## What counts as a security issue

The engine evaluates untrusted contract terms, for example JSON pasted into
the ACTUS Explorer, which runs the engine as WebAssembly. Any of these is in
scope:

- **Panics.** Any input that makes `actus-wasm` or `actus-engine` panic. The
  WASM build aborts on panic, which crashes the web UI.
- **Unbounded work.** Inputs that make evaluation hang or use unbounded
  memory or time.
- **Unsafe or dependency issues.** Memory-safety issues, or vulnerable
  dependencies that `cargo deny check` does not already report.

Wrong financial results are correctness bugs. Report those as normal issues
with a reproduction.
