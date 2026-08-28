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

type CodeqlAnalyzeJob = {
  "continue-on-error"?: boolean;
  if?: string;
  permissions?: Record<string, string>;
  "runs-on"?: string;
  strategy?: {
    "fail-fast"?: boolean;
    matrix?: { include?: MatrixEntry[] };
  };
  steps?: WorkflowStep[];
};

type CodeqlWorkflow = {
  name?: string;
  on?: {
    workflow_dispatch?: null;
    push?: { branches?: string[] };
    pull_request?: { branches?: string[]; types?: string[] };
    merge_group?: { types?: string[] };
    schedule?: Array<{ cron?: string }>;
  };
  jobs?: Record<string, unknown> & { analyze?: CodeqlAnalyzeJob };
};

const parsedWorkflow = parse(codeqlWorkflow) as CodeqlWorkflow;

describe("Official CodeQL workflow governance", () => {
  it("preserves the complete CodeQL event contract", () => {
    expect(parsedWorkflow.name).toBe("CodeQL");
    expect(parsedWorkflow.on).toEqual({
      workflow_dispatch: null,
      push: { branches: ["main"] },
      pull_request: {
        branches: ["main"],
        types: ["opened", "reopened", "synchronize", "ready_for_review"],
      },
      merge_group: { types: ["checks_requested"] },
      schedule: [{ cron: "19 7 * * 2" }],
    });
  });

  it("pins the compatible Rust sysroot only for the Rust matrix cell", () => {
    expect(parsedWorkflow.jobs?.analyze?.strategy).toEqual({
      "fail-fast": false,
      matrix: {
        include: [
          { language: "actions", "build-mode": "none" },
          { language: "javascript-typescript", "build-mode": "none" },
          { language: "rust", "build-mode": "none" },
        ],
      },
    });

    const steps = parsedWorkflow.jobs?.analyze?.steps ?? [];
    const checkoutStepIndex = steps.findIndex((step) => step.uses?.startsWith("actions/checkout@"));
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
    const sysrootConfigurationSteps = steps.filter((step) =>
      JSON.stringify(step).includes("CODEQL_EXTRACTOR_RUST_OPTION_SYSROOT"),
    );

    expect(steps).toHaveLength(4);
    expect(checkoutStepIndex).toBeGreaterThanOrEqual(0);
    expect(rustStepIndex).toBeGreaterThan(checkoutStepIndex);
    expect(initializeStepIndex).toBeGreaterThan(rustStepIndex);
    expect(analyzeStepIndex).toBeGreaterThan(initializeStepIndex);
    expect(sysrootConfigurationSteps).toHaveLength(1);
    expect(sysrootConfigurationSteps[0]).toBe(rustStep);
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
      "github/codeql-action/init@cdf488f595d80d6e07e03d4674febd5ab45fa938",
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

  it("uses one pinned checkout without persisting credentials", () => {
    const checkoutSteps = (parsedWorkflow.jobs?.analyze?.steps ?? []).filter((step) =>
      step.uses?.startsWith("actions/checkout@"),
    );
    const [checkoutStep] = checkoutSteps;

    expect(checkoutSteps).toHaveLength(1);
    expect(checkoutStep?.uses).toBe("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1");
    expect(checkoutStep?.if).toBeUndefined();
    expect(checkoutStep?.["continue-on-error"] ?? false).toBe(false);
    expect(checkoutStep?.with).toEqual({ "persist-credentials": false });
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
    expect(analyzeJob?.["runs-on"]).toBe("ubuntu-latest");
    expect(analyzeJob?.permissions).toEqual({
      actions: "read",
      contents: "read",
      "security-events": "write",
    });
    expect(analyzeStep).toBeDefined();
    expect(analyzeStep?.name).toBe("Perform CodeQL analysis");
    expect(analyzeStep?.uses).toBe(
      "github/codeql-action/analyze@cdf488f595d80d6e07e03d4674febd5ab45fa938",
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

  it("keeps the retired custom SARIF and legacy analysis paths absent", () => {
    expect(Object.keys(parsedWorkflow.jobs ?? {})).toEqual(["analyze"]);
    expect(JSON.stringify(parsedWorkflow)).not.toContain("codeql-sarif-gate");
  });
});
