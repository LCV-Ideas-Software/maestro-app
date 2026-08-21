import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MainSiteD1PublishPlan } from "../../types";
import { MainSitePublishingPanel } from "./MainSitePublishingPanel";

const plan = {
  plan_id: "plan-1",
  action: "update",
  sql_intent: "UPDATE mainsite_posts com parametros; bump content-version",
  content_version_current: 10,
  content_version_next: 11,
  diff_summary: [{ field: "content", change: "changed" }],
} as MainSiteD1PublishPlan;

describe("MainSitePublishingPanel", () => {
  it("never enables a remote write before explicit confirmation", () => {
    const publish = vi.fn();
    render(
      <MainSitePublishingPanel
        busy={false}
        error={null}
        plan={plan}
        result={null}
        targetConfigured
        onPreview={vi.fn()}
        onPublish={publish}
        onReset={vi.fn()}
      />,
    );

    const button = screen.getByRole("button", { name: "Publicar e confirmar readback" });
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    expect(button).toBeEnabled();
    fireEvent.click(button);
    expect(publish).toHaveBeenCalledOnce();
  });

  it("labels preview as zero-write and blocks it without a target", () => {
    render(
      <MainSitePublishingPanel
        busy={false}
        error={null}
        plan={null}
        result={null}
        targetConfigured={false}
        onPreview={vi.fn()}
        onPublish={vi.fn()}
        onReset={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Gerar preview sem escrita" })).toBeDisabled();
    expect(screen.getByText(/nao executa nenhuma escrita/i)).toBeInTheDocument();
  });
});
