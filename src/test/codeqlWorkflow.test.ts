/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";

import rustExtractionNote from "../../.github/CODEQL_RUST_EXTRACTION.md?raw";
import codeqlWorkflow from "../../.github/workflows/codeql.yml?raw";

describe("Official CodeQL workflow governance", () => {
  it("covers pull-request transitions and merge-queue checks", () => {
    expect(codeqlWorkflow).toMatch(
      /pull_request:[\s\S]*types:[\s\S]*- opened[\s\S]*- reopened[\s\S]*- synchronize[\s\S]*- ready_for_review/,
    );
    expect(codeqlWorkflow).toMatch(/merge_group:\s*\n\s+types:\s*\n\s+- checks_requested/);
  });

  it("pins the compatible Rust sysroot only for the Rust matrix cell", () => {
    expect(codeqlWorkflow).toMatch(
      /name: Install CodeQL-compatible Rust sysroot[\s\S]*if: matrix\.language == 'rust'[\s\S]*rustup toolchain install 1\.94\.0 --profile minimal --component rust-src/,
    );
    expect(codeqlWorkflow).toContain("rustup run 1.94.0 rustc --print sysroot");
    expect(codeqlWorkflow).toContain("CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT=$sysroot");
    expect(codeqlWorkflow).toContain(
      "CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT_SRC=$sysroot/lib/rustlib/src/rust/library",
    );
    expect(codeqlWorkflow).toContain('>> "$GITHUB_ENV"');
    expect(codeqlWorkflow).not.toContain("rustup default");
  });

  it("documents the bounded Rust extractor limitation without suppressing findings", () => {
    expect(rustExtractionNote).toContain("src-tauri/src/lib.rs:800:14");
    expect(rustExtractionNote).toContain("expected expression");
    expect(rustExtractionNote).toContain(
      "macro expansion failed: the macro `tauri::generate_context` expands to ERROR but a Expr was expected",
    );
    expect(rustExtractionNote).toContain("1 of 40 repository Rust source files");
    expect(rustExtractionNote).toContain(
      "https://docs.github.com/en/code-security/reference/code-scanning/troubleshoot-analysis-errors/extraction-errors-in-the-database",
    );
    expect(rustExtractionNote).toContain(
      "https://docs.github.com/en/code-security/reference/code-scanning/codeql/build-options-for-compiled-languages",
    );
    expect(rustExtractionNote).toContain("https://github.com/rust-lang/rust-analyzer/issues/12803");
    expect(rustExtractionNote).toContain("actions/runs/31290510940/job/93188784129");
    expect(codeqlWorkflow).toContain("../CODEQL_RUST_EXTRACTION.md");
    expect(codeqlWorkflow).not.toContain("export-diagnostics");
  });
});
