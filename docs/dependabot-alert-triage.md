# Dependabot Alert Triage

Status: active security register.
Scope: `src-tauri/Cargo.lock`.

## Current checkpoint - 05/10/2026

The native repository read found zero open Dependabot alerts. Final local
Cargo audit found zero active vulnerabilities, retaining the Linux-only glib
unsoundness and proc-macro-error maintenance warnings. OSV-Scanner 2.6.0
found 454 locked packages. Eight earlier GTK advisories were officially
withdrawn on 14/08/2026, as confirmed through their native OSV records.
Those obsolete exceptions were removed together with the five absent UNIC
dependencies. The two remaining Linux-only exceptions retain 03/11/2026.
These local results do not certify GitHub checks or a published artifact.

## Historical Rust alert decisions

These records describe earlier graphs. Their alert numbers and old Tauri
versions must not be read as current open findings.

### GHSA-wrw7-89jp-8q8g - `glib`

- Dependabot alert: `#1`.
- Severity: medium.
- Vulnerable range: `>= 0.15.0, < 0.20.0`.
- Patched version: `0.20.0`.
- Dependency path: Tauri/Wry Linux GTK/WebKit stack.
- Supported Maestro target: Windows 11+.
- Evidence:
  - `cargo tree -i glib@0.18.5 --target x86_64-pc-windows-msvc` prints no dependency path.
  - `cargo tree -i glib@0.18.5 --target all` shows the dependency through GTK/WebKit Linux crates.
  - Release workflow builds only the Windows portable executable.
- Triage decision: vulnerable code is not used by the supported Windows runtime.

### GHSA-cq8v-f236-94qc - `rand`

- Dependabot alert: `#2`.
- Severity: low.
- Vulnerable range: `>= 0.7.0, < 0.8.6`.
- Patched version: `0.8.6`.
- Dependency path: `tauri-utils -> kuchikiki -> selectors -> phf_codegen -> phf_generator -> rand@0.7.3`.
- Dependency role: build-time transitive dependency from Tauri's HTML manipulation/code generation path.
- Evidence:
  - `cargo update --dry-run` reports no compatible update for the current Tauri 2.10.3 dependency set.
  - `cargo tree -e features -i rand@0.7.3 --target x86_64-pc-windows-msvc` shows `rand@0.7.3` through build dependencies, not Maestro application code.
  - `rg "rand::|thread_rng|rng\\(|impl log::Log|set_logger|env_logger|tracing_subscriber|log::set" src-tauri src` finds no Maestro code path using `rand` or a custom logger.
  - The advisory requires a custom logger that calls `rand::rng()`/`thread_rng()` under specific reseeding and logging conditions.
- Triage decision: tolerable transitive build-time risk until Tauri's dependency graph ships a compatible patched path.

## Local Hardening Applied

- Tauri default features are disabled.
- Maestro enables only the Tauri features needed for the Windows WebView runtime:
  - `common-controls-v6`
  - `dynamic-acl`
  - `wry`
- This removes unnecessary X11 crates from the lockfile and keeps the supported build surface aligned with Windows 11+.

## Scorecard / OSV Scanner Triage

OpenSSF Scorecard SARIF can report RustSec/OSV advisories from every package recorded in `src-tauri/Cargo.lock`, including cross-platform dependencies that do not resolve for the shipped Windows target.

The 05/10/2026 re-evaluation of the stable locked graph reports no active
Cargo advisories. The previous five UNIC exceptions no longer apply:
Tauri 2.12.0 resolves tauri-runtime-wry 2.12.1, Wry 0.57.0 and
tauri-utils 2.10.1, which selects urlpattern 0.6.0 without rust-unic.
This stable change satisfies the MAESTRO-1 re-evaluation trigger.

- `cargo audit --file src-tauri/Cargo.lock --json`: zero active vulnerabilities.
- Remaining warning categories: one unmaintained macro dependency
  (`proc-macro-error`) and one unsound Linux dependency (`glib`).
- `cargo tree --locked --target x86_64-pc-windows-msvc -i gtk`,
  `-i glib` and `-i proc-macro-error`: no dependency path.
- `cargo tree --locked --target all -i gtk` and `-i proc-macro-error`:
  the Linux GTK/WebKit path remains in the cross-platform lock.
- The yanked `yoke-derive 0.8.3` resolution was replaced with the compatible
  published `0.8.4` using the official Cargo updater.

`src-tauri/osv-scanner.toml` retains two exceptions, each with its existing
`ignoreUntil = 2026-11-03` and target-specific reason. The five obsolete
rust-unic exceptions and eight withdrawn GTK advisories were removed;
historical bundled license text is retained.

- Linux glib unsoundness: `RUSTSEC-2024-0429`.
- GTK macro transitive: `RUSTSEC-2024-0370`.

Upstream references: [stable Tauri 2.12.0](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.0),
[published tauri-utils 2.10.1 manifest](https://docs.rs/crate/tauri-utils/2.10.1/source/Cargo.toml),
[published Wry 0.57.0 manifest](https://docs.rs/crate/wry/0.57.0/source/Cargo.toml)
and [RustSec glib advisory](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).

The GTK4/WebKitGTK 6 migration remains a Tauri v3 platform change. Preserve the
remaining Linux exceptions until a supported stable graph fixes their premises
or Linux support is separately assessed. Do not infer that removing the UNIC
chain also removed the Linux GTK advisories.

## Follow-up Policy

- Keep Dependabot Cargo updates enabled for `/src-tauri`.
- Reopen dismissed alerts if Tauri publishes a compatible dependency path that removes the vulnerable transitive crate.
- Re-evaluate the stable Tauri/Wry graph no later than 03/11/2026, even if no upstream release notification arrives.
- Do not publish Linux builds until the GTK/WebKit `glib` path is upgraded or separately triaged for that target.
