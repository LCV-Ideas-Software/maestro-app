/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";

import workflow from "../../.github/workflows/dependabot-automerge.yml?raw";

describe("Dependabot Automerge workflow", () => {
  it("does not update Dependabot branches with GITHUB_TOKEN after validating required checks", () => {
    expect(workflow).not.toContain("gh pr update-branch");
    expect(workflow).toContain("@dependabot rebase");
    expect(workflow).toContain("@dependabot recreate");
  });

  it("waits for repository-required checks on the current head before merging", () => {
    expect(workflow).toContain('"Repository hygiene"');
    expect(workflow).toContain('"Rust gates (cargo --locked)"');
    expect(workflow).toContain('"Check index.html formatting"');
  });

  it("treats CodeQL workflow checks as CodeQL even when the check name is matrix-specific", () => {
    expect(workflow).toContain('.workflowName == "CodeQL"');
  });
});
