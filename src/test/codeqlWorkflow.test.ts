/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";
import { parse } from "yaml";

import rustExtractionNote from "../../.github/CODEQL_RUST_EXTRACTION.md?raw";
import codeqlWorkflow from "../../.github/workflows/codeql.yml?raw";

type WorkflowStep = {
  "continue-on-error"?: boolean;
  name?: string;
  if?: string;
  run?: string;
  shell?: string;
  uses?: string;
  with?: Record<string, boolean | string>;
};

type MatrixEntry = {
  language?: string;
  "build-mode"?: string;
};

type CodeqlWorkflow = {
  name?: string;
  on?: {
    push?: { branches?: string[] };
    pull_request?: { branches?: string[]; types?: string[] };
    merge_group?: { types?: string[] };
  };
  jobs?: {
    analyze?: {
      "continue-on-error"?: boolean;
      if?: string;
      strategy?: { matrix?: { include?: MatrixEntry[] } };
      steps?: WorkflowStep[];
    };
  };
};

const parsedWorkflow = parse(codeqlWorkflow) as CodeqlWorkflow;

describe("Official CodeQL workflow governance", () => {
  it("covers pull-request transitions and merge-queue checks", () => {
    expect(parsedWorkflow.name).toBe("CodeQL");
    expect(parsedWorkflow.on?.push?.branches).toEqual(["main"]);
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
    expect(parsedWorkflow.jobs?.analyze?.strategy?.matrix?.include).toEqual([
      { language: "actions", "build-mode": "none" },
      { language: "javascript-typescript", "build-mode": "none" },
      { language: "rust", "build-mode": "none" },
    ]);

    const steps = parsedWorkflow.jobs?.analyze?.steps ?? [];
    const rustStepIndex = steps.findIndex(
      (step) => step.name === "Install CodeQL-compatible Rust sysroot",
    );
    const initializeStepIndex = steps.findIndex((step) =>
      step.uses?.startsWith("github/codeql-action/init@"),
    );
    const analyzeStepIndex = steps.findIndex((step) =>
      step.uses?.startsWith("github/codeql-action/analyze@"),
    );
    const rustStep = steps[rustStepIndex];

    expect(rustStepIndex).toBeGreaterThanOrEqual(0);
    expect(initializeStepIndex).toBeGreaterThan(rustStepIndex);
    expect(analyzeStepIndex).toBeGreaterThan(initializeStepIndex);
    expect(rustStep).toBeDefined();
    expect(rustStep?.if).toBe("matrix.language == 'rust'");
    expect(rustStep?.["continue-on-error"] ?? false).toBe(false);
    expect(rustStep?.shell).toBe("bash");
    const activeRunLines = (rustStep?.run ?? "")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith("#"));
    expect(activeRunLines).toEqual([
      "set -euo pipefail",
      "rustup toolchain install 1.94.0 --profile minimal --component rust-src",
      'sysroot="$(rustup run 1.94.0 rustc --print sysroot)"',
      'test -d "$sysroot/lib/rustlib/src/rust/library"',
      "printf '%s\\n' \\",
      '"CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT=$sysroot" \\',
      '"CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT_SRC=$sysroot/lib/rustlib/src/rust/library" \\',
      '>> "$GITHUB_ENV"',
    ]);
    expect(rustStep?.run).not.toContain("rustup default");
  });

  it("initializes CodeQL with the pinned matrix inputs before analysis", () => {
    const steps = parsedWorkflow.jobs?.analyze?.steps ?? [];
    const initializeSteps = steps.filter((step) =>
      step.uses?.startsWith("github/codeql-action/init@"),
    );
    const analyzeStepIndex = steps.findIndex((step) =>
      step.uses?.startsWith("github/codeql-action/analyze@"),
    );
    const [initializeStep] = initializeSteps;
    const initializeStepIndex = steps.findIndex((step) =>
      step.uses?.startsWith("github/codeql-action/init@"),
    );

    expect(initializeSteps).toHaveLength(1);
    expect(initializeStepIndex).toBeGreaterThanOrEqual(0);
    expect(analyzeStepIndex).toBeGreaterThan(initializeStepIndex);
    expect(initializeStep?.uses).toBe(
      "github/codeql-action/init@ff2f1c621b7f889edc0d3c761ac2e6a3f8cdb0dd",
    );
    expect(initializeStep?.if).toBeUndefined();
    expect(initializeStep?.["continue-on-error"] ?? false).toBe(false);
    expect(initializeStep?.with).toEqual({
      languages: ["$", "{{ matrix.language }}"].join(""),
      "build-mode": ["$", "{{ matrix.build-mode }}"].join(""),
      queries: "security-and-quality",
      "dependency-caching": true,
    });
  });

  it("keeps the pinned official analyzer and per-language category active", () => {
    const analyzeJob = parsedWorkflow.jobs?.analyze;
    const analyzeSteps = (analyzeJob?.steps ?? []).filter((step) =>
      step.uses?.startsWith("github/codeql-action/analyze@"),
    );
    const [analyzeStep] = analyzeSteps;

    expect(analyzeSteps).toHaveLength(1);
    expect(analyzeJob?.if).toBeUndefined();
    expect(analyzeJob?.["continue-on-error"] ?? false).toBe(false);
    expect(analyzeStep).toBeDefined();
    expect(analyzeStep?.name).toBe("Perform CodeQL analysis");
    expect(analyzeStep?.uses).toBe(
      "github/codeql-action/analyze@ff2f1c621b7f889edc0d3c761ac2e6a3f8cdb0dd",
    );
    expect(analyzeStep?.if).toBeUndefined();
    expect(analyzeStep?.["continue-on-error"] ?? false).toBe(false);
    expect(analyzeStep?.with?.category).toBe(["/language:", "$", "{{ matrix.language }}"].join(""));
    expect(analyzeStep?.with?.upload ?? "always").toBe("always");
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
