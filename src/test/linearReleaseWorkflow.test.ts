/// <reference types="vite/client" />

import { describe, expect, it } from "vitest";
import { parse } from "yaml";

import actionsLockSource from "../../.github/workflows/actions.lock?raw";
import workflowSource from "../../.github/workflows/linear-release.yml?raw";
import agentsGuide from "../../AGENTS.md?raw";

const LINEAR_ACTION_SHA = "0a25abab892a91062ebf42260dbb2ce6277aa205";
const CHECKOUT_SHA = "3d3c42e5aac5ba805825da76410c181273ba90b1";

type WorkflowStep = {
  "continue-on-error"?: boolean;
  id?: string;
  name?: string;
  uses?: string;
  with?: Record<string, boolean | number | string>;
};

type LinearReleaseWorkflow = {
  name?: string;
  on?: { push?: { branches?: string[] } };
  permissions?: Record<string, string>;
  concurrency?: {
    group?: string;
    queue?: string;
    "cancel-in-progress"?: boolean;
  };
  jobs?: {
    linear_release?: {
      environment?: string;
      permissions?: Record<string, string>;
      steps?: WorkflowStep[];
    };
  };
};

type ActionsLock = {
  workflows?: Record<string, string[]>;
  dependencies?: Record<
    string,
    { commit?: string; owner_id?: number; ref?: string; repo_id?: number }
  >;
};

const workflow = parse(workflowSource) as LinearReleaseWorkflow;
const actionsLock = parse(actionsLockSource) as ActionsLock;

describe("Official Linear Release workflow", () => {
  it("preserves the continuous pipeline trigger and least-privilege boundary", () => {
    expect(workflow.name).toBe("Linear Release");
    expect(workflow.on).toEqual({ push: { branches: ["main"] } });
    expect(workflow.permissions).toEqual({});
    expect(workflow.concurrency).toEqual({
      group: ["linear-release-", "$", "{{ github.ref }}"].join(""),
      queue: "max",
    });
    expect(workflow.jobs?.linear_release?.environment).toBe("linear-release");
    expect(workflow.jobs?.linear_release?.permissions).toEqual({
      contents: "read",
    });
  });

  it("uses only full-history checkout and the exact official action pin", () => {
    const steps = workflow.jobs?.linear_release?.steps ?? [];

    expect(steps).toHaveLength(2);
    expect(steps[0]).toEqual({
      uses: `actions/checkout@${CHECKOUT_SHA}`,
      with: { "fetch-depth": 0, "persist-credentials": false },
    });
    expect(steps[1]).toEqual({
      id: "linear_release",
      name: "Create Linear release with the official action",
      uses: `linear/linear-release-action@${LINEAR_ACTION_SHA}`,
      with: {
        access_key: ["$", "{{ secrets.LINEAR_ACCESS_KEY }}"].join(""),
        cli_version: "v0.16.0",
      },
    });
    expect(workflowSource).not.toContain("continue-on-error:");
    expect(workflowSource).not.toContain("linear-release-linux-x64");
    expect(workflowSource).not.toContain("CLI_SHA256");
  });

  it("locks the direct action to the signed v0.16.0 commit", () => {
    const dependency = `linear/linear-release-action@${LINEAR_ACTION_SHA}`;

    expect(actionsLock.workflows?.[".github/workflows/linear-release.yml"]).toEqual([
      `actions/checkout@${CHECKOUT_SHA}`,
      dependency,
    ]);
    expect(actionsLock.dependencies?.[dependency]).toEqual({
      ref: "v0.16.0",
      commit: `sha1-${LINEAR_ACTION_SHA}`,
      owner_id: 46686594,
      repo_id: 1150447766,
    });
  });

  it("names only the current cross-review service", () => {
    expect(agentsGuide).toContain("single `cross-review` service");
    expect(agentsGuide).not.toMatch(/cross-review-v[12]/);
  });
});
