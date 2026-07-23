/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";

import workflow from "../../.github/workflows/dependabot-automerge.yml?raw";

describe("Dependabot Automerge workflow", () => {
  it("delegates branch updates and merges to the pinned guarded controller", () => {
    expect(workflow).not.toContain("gh pr update-branch");
    expect(workflow).not.toContain("@dependabot rebase");
    expect(workflow).toContain(
      "LCV-Ideas-Software/.github/dependabot-automerge@06078610da5edd1c6d020db205c69d3cd580fcf9",
    );
    expect(workflow).toContain("queue: max");
  });

  it("binds repository-required checks to immutable GitHub App IDs", () => {
    expect(workflow).toContain('{"name":"Repository hygiene","app_id":15368}');
    expect(workflow).toContain('{"name":"Rust gates (cargo --locked)","app_id":15368}');
    expect(workflow).toContain('{"name":"Check index.html formatting","app_id":15368}');
  });

  it("requires CodeQL and zizmor results from GitHub Advanced Security", () => {
    expect(workflow).toContain('{"name":"CodeQL","app_id":57789}');
    expect(workflow).toContain('{"name":"zizmor","app_id":57789}');
    expect(workflow).toContain("- Zizmor");
  });
});
