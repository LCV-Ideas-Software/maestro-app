# CodeQL Rust extraction boundary

## Current setup — 08/09/2026

The repository uses GitHub CodeQL Default Setup for Actions,
JavaScript/TypeScript and Rust, with the extended query suite and standard
GitHub-hosted runners. The repository-owned advanced workflow is retired;
an explicit repository Rust sysroot is not required by the current setup.

At commit `b79a7060ed9c1a9d0b0566495cd59b7e3bab1b80`, native Default Setup Rust
analysis `1740961198` used analysis key
`dynamic/github-code-scanning/codeql:analyze` and category `/language:rust`,
with 27 rules, zero results and an empty analysis error. This is an exact-head
observation, not a promise about future analyses. The separate Windows CI job
continues to run the product's locked Rust checks, tests and Clippy.

GitHub documents Rust support in Default Setup using build mode `none`:
[CodeQL for compiled languages](https://docs.github.com/en/code-security/concepts/code-scanning/codeql/codeql-for-compiled-languages).

## Historical incident — 08/08/2026

The following accepted platform diagnostic is preserved as historical evidence,
not a current requirement to restore Advanced Setup or an application defect.

### Exact audited evidence

At pull request head
`657062704a186953b29c0622cf9beb967f058a72`, the Rust job in
[CodeQL run 31290510940](https://github.com/LCV-Ideas-Software/maestro-app/actions/runs/31290510940/job/93188784129)
completed successfully with zero SARIF results. After the workflow supplied a
Rust 1.94.0 sysroot and `rust-src`, its only repository-local extractor warning
was one source location in 1 of 40 repository Rust source files:

- `src-tauri/src/lib.rs:800:14`: `expected expression`
- `src-tauri/src/lib.rs:800:14`: ``macro expansion failed: the macro `tauri::generate_context` expands to ERROR but a Expr was expected``

The repository's independent `cargo --locked` gates passed on the same pull
request head. No application source change is justified by this extractor-only
warning.

### Platform boundary

GitHub documents that CodeQL Rust `build-mode: none` uses `rust-analyzer` to
compile build scripts and macro code without invoking a full build. GitHub also
documents that a small number of extractor errors is healthy, that most do not
significantly affect analysis, and that investigation is required when errors
affect the overwhelming majority of compiled files:

- [CodeQL build options for compiled languages](https://docs.github.com/en/code-security/reference/code-scanning/codeql/build-options-for-compiled-languages)
- [Extraction errors in the database](https://docs.github.com/en/code-security/reference/code-scanning/troubleshoot-analysis-errors/extraction-errors-in-the-database)

The upstream `rust-analyzer` project describes why procedural-macro expansion
depends on an inherently unstable bridge between its proc-macro server and the
Rust compiler. This is architectural context for the residual macro-expansion
limitation, not a claim that the repository reproduces every condition in that
issue:

- [rust-analyzer issue #12803](https://github.com/rust-lang/rust-analyzer/issues/12803)

## Enforcement

This record does not suppress or baseline any CodeQL result. The inherited
Enterprise code-scanning rule remains authoritative for pull requests and the
default branch. There is no merge queue, repository-owned SARIF parser, log
parser or release-readiness polling gate. Native Default Setup and the separate
Windows Rust job have distinct responsibilities; neither historical extractor
warnings nor this note waive actionable current findings.

This evidence is specific to the exact run, head, location, and messages above.
Any change in that evidence requires a new investigation; it is not pre-approved
by this record.
