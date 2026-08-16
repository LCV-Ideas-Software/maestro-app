# CodeQL Rust extraction boundary

Status: accepted platform diagnostic, not an application defect or a CodeQL
finding.

## Exact audited evidence

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

## Platform boundary

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

This record does not suppress or baseline any CodeQL result. The repository uses
GitHub's official CodeQL Advanced Setup for Actions, JavaScript/TypeScript and
Rust, while the Enterprise code-scanning rule protects pull requests and the
default branch. Exact-head analyses and open alerts are checked before queue
admission; the separate Rust job enforces the complete `cargo --locked` gates.
No repository-owned SARIF parser or log parser is used. GitHub's documented
merge-group boundary for code-scanning merge protection is an accepted platform
limit under the organization-wide native-controls policy, not a waiver for an
open alert.

This evidence is specific to the exact run, head, location, and messages above.
Any change in that evidence requires a new investigation; it is not pre-approved
by this record.
