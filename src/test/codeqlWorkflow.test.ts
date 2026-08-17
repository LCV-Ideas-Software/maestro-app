/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";
import { parse } from "yaml";

import rustExtractionNote from "../../.github/CODEQL_RUST_EXTRACTION.md?raw";
import codeqlWorkflow from "../../.github/workflows/codeql.yml?raw";

type WorkflowStep = {
  name?: string;
  if?: string;
  run?: string;
  uses?: string;
  with?: Record<string, string>;
};

type MatrixEntry = {
  language?: string;
  "build-mode"?: string;
};

type CodeqlWorkflow = {
  on?: {
    pull_request?: { branches?: string[]; types?: string[] };
    merge_group?: { types?: string[] };
  };
  jobs?: {
    analyze?: {
      strategy?: { matrix?: { include?: MatrixEntry[] } };
      steps?: WorkflowStep[];
    };
  };
};

const parsedWorkflow = parse(codeqlWorkflow) as CodeqlWorkflow;

describe("Official CodeQL workflow governance", () => {
  it("covers pull-request transitions and merge-queue checks", () => {
    expect(parsedWorkflow.on?.pull_request?.branches).toEqual(["main"]);
    expect(parsedWorkflow.on?.pull_request?.types).toEqual([
      "opened",
      "reopened",
      "synchronize",
      "ready_for_review",
    ]);
    expect(parsedWorkflow.on?.merge_group?.types).toEqual(["checks_requested"]);
  });

  it("pins the compatible Rust sysroot only for the Rust matrix cell", () => {
    expect(parsedWorkflow.jobs?.analyze?.strategy?.matrix?.include).toContainEqual({
      language: "rust",
      "build-mode": "none",
    });

    const steps = parsedWorkflow.jobs?.analyze?.steps ?? [];
    const rustStepIndex = steps.findIndex(
      (step) => step.name === "Install CodeQL-compatible Rust sysroot",
    );
    const analyzeStepIndex = steps.findIndex((step) => step.name === "Perform CodeQL analysis");
    const rustStep = steps[rustStepIndex];

    expect(rustStepIndex).toBeGreaterThanOrEqual(0);
    expect(analyzeStepIndex).toBeGreaterThan(rustStepIndex);
    expect(rustStep).toBeDefined();
    expect(rustStep?.if).toBe("matrix.language == 'rust'");
    expect(rustStep?.run).toContain(
      "rustup toolchain install 1.94.0 --profile minimal --component rust-src",
    );
    expect(rustStep?.run).toContain("rustup run 1.94.0 rustc --print sysroot");
    expect(rustStep?.run).toContain("CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT=$sysroot");
    expect(rustStep?.run).toContain(
      "CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT_SRC=$sysroot/lib/rustlib/src/rust/library",
    );
    expect(rustStep?.run).toContain('>> "$GITHUB_ENV"');
    expect(rustStep?.run).not.toContain("rustup default");
  });

  it("keeps the pinned official analyzer and per-language category active", () => {
    const analyzeStep = parsedWorkflow.jobs?.analyze?.steps?.find(
      (step) => step.name === "Perform CodeQL analysis",
    );

    expect(analyzeStep).toBeDefined();
    expect(analyzeStep?.uses).toBe(
      "github/codeql-action/analyze@ff2f1c621b7f889edc0d3c761ac2e6a3f8cdb0dd",
    );
    expect(analyzeStep?.with?.category).toBe(["/language:", "$", "{{ matrix.language }}"].join(""));
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
