/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";

import rustExtractionNote from "../../.github/CODEQL_RUST_EXTRACTION.md?raw";
import codeqlWorkflow from "../../.github/workflows/codeql.yml?raw";
import workflow from "../../.github/workflows/native-auto-merge.yml?raw";
import zizmorWorkflow from "../../.github/workflows/zizmor.yml?raw";

describe("Native Auto-merge workflow", () => {
  it("delegates only native auto-merge arming to the pinned central action", () => {
    expect(workflow).toContain(
      "LCV-Ideas-Software/.github/native-auto-merge@4058fad11eca7c2eb4e9296108667ef6199a6356",
    );
    expect(workflow).toContain("environment: dependabot-automation");
    expect(workflow).toContain(
      ["automation_token: $", "{{ secrets.LCV_AUTOMATION_TOKEN }}"].join(""),
    );
    expect(workflow).not.toContain("dependabot-automerge@");
    expect(workflow).not.toContain("gh pr update-branch");
    expect(workflow).not.toContain("@dependabot rebase");
  });

  it("uses the pinned v2 central wrapper for Zizmor", () => {
    expect(zizmorWorkflow).toContain(
      "LCV-Ideas-Software/.github/.github/workflows/zizmor.yml@4058fad11eca7c2eb4e9296108667ef6199a6356 # v2.0.0",
    );
    expect(zizmorWorkflow).not.toContain("# v1.0.2");
  });

  it("runs only after CodeQL pull-request completion", () => {
    expect(workflow).toMatch(/workflows:\s*\n\s+- CodeQL\s*\n\s+types:/);
    expect(workflow).toContain("github.event.workflow_run.event == 'pull_request'");
    expect(workflow).not.toMatch(/schedule:|workflow_dispatch:|actions\/checkout/);
  });

  it("passes all six explicit workflow-run event inputs", () => {
    for (const input of [
      "event_repository",
      "workflow_name",
      "workflow_status",
      "workflow_event",
      "workflow_head_sha",
      "workflow_pull_requests",
    ]) {
      expect(workflow).toContain(`${input}:`);
    }
    expect(workflow).not.toContain("required_checks_json:");
    expect(workflow).not.toContain("settle_timeout_seconds:");
  });

  it("reruns CodeQL when a draft becomes ready", () => {
    expect(codeqlWorkflow).toMatch(
      /pull_request:[\s\S]*types:[\s\S]*- opened[\s\S]*- reopened[\s\S]*- synchronize[\s\S]*- ready_for_review/,
    );
    expect(codeqlWorkflow).toMatch(/merge_group:\s*\n\s+types:/);
  });

  it("pins a compatible CodeQL Rust sysroot only for the Rust matrix cell", () => {
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

  it("documents the exact bounded Rust extractor limitation without weakening the SARIF gate", () => {
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

  it("enforces both CodeQL categories through the immutable strict SARIF action", () => {
    expect(
      codeqlWorkflow.match(
        /uses: LCV-Ideas-Software\/\.github\/codeql-sarif-gate@24b0bcc09a48b47f740b8a8bd972374f7289e48e # codeql-sarif-v1\.0\.0/g,
      ),
    ).toHaveLength(2);
    expect(
      codeqlWorkflow.match(
        /uses: LCV-Ideas-Software\/\.github\/codeql-sarif-gate@24b0bcc09a48b47f740b8a8bd972374f7289e48e # codeql-sarif-v1\.0\.0\r?\n\s+with:\r?\n\s+sarif-directory: \$\{\{ runner\.temp \}\}\/codeql-results/g,
      ),
    ).toHaveLength(2);
    expect(codeqlWorkflow).not.toMatch(
      /mapfile\s+-d|find\s+"\$CODEQL_RESULTS"|CODEQL_RESULTS:|finding_count|jq\s+-s\s+'\[\.\[\]\.runs/,
    );
  });
});
