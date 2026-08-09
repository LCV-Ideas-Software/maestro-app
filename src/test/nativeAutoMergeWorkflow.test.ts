/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";

import codeqlWorkflow from "../../.github/workflows/codeql.yml?raw";
import workflow from "../../.github/workflows/native-auto-merge.yml?raw";

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
});
