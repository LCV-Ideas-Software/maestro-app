import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listLinkIntegrityRecords } from "../../services/evidence";
import type { LinkIntegrityRecord } from "../../types";
import { LinkIntegrityPanel } from "./LinkIntegrityPanel";

vi.mock("../../services/evidence", () => ({
  listLinkIntegrityRecords: vi.fn(),
  proposeLinkCorrections: vi.fn(),
  reviewLinkIntegrity: vi.fn(),
}));

const listMock = vi.mocked(listLinkIntegrityRecords);

function linkRecord(overrides: Partial<LinkIntegrityRecord> = {}): LinkIntegrityRecord {
  return {
    schema_version: "link_evidence.v1",
    link_id: "matching-link",
    source_artifact: "operator/mainsite-posteditor",
    source_fingerprint: "fingerprint",
    anchor_text: "matching record",
    surrounding_text: "Editorial context",
    original_url: "https://example.com/original",
    normalized_url: "https://example.com/original",
    normalization_changes: [],
    final_url: null,
    redirect_chain: [],
    http_status: 404,
    content_type: null,
    sha256: null,
    checked_at: "2026-10-05T00:00:00Z",
    claim_supported: null,
    classification: "not_found",
    correction_candidates: [],
    cross_review_status: "pending",
    review_decision: null,
    reviewed_by: null,
    review_note: "NEEDLE preserved in the review note",
    reviewed_at: null,
    web_evidence_id: null,
    url: "https://example.com/original",
    status: "NOT_FOUND",
    invalidity: "Not found",
    tone: "warn",
    ...overrides,
  };
}

describe("LinkIntegrityPanel", () => {
  beforeEach(() => listMock.mockReset());

  it("starts origin pagination from a fresh server query", async () => {
    listMock
      .mockResolvedValueOnce({ items: [], next_cursor: "old-cursor", total: 31 })
      .mockResolvedValueOnce({ items: [], next_cursor: "new-cursor", total: 12 })
      .mockResolvedValueOnce({ items: [], next_cursor: null, total: 12 });

    render(<LinkIntegrityPanel recentRecords={[]} />);
    await screen.findByRole("button", { name: /Carregar mais \(0 de 31\)/ });

    fireEvent.change(screen.getByRole("combobox", { name: "Filtrar por origem do link" }), {
      target: { value: "operator/mainsite-posteditor" },
    });
    expect(listMock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Aplicar filtros" }));
    await screen.findByRole("button", { name: /Carregar mais \(0 de 12\)/ });

    fireEvent.click(screen.getByRole("button", { name: /Carregar mais \(0 de 12\)/ }));
    await waitFor(() => expect(listMock).toHaveBeenCalledTimes(3));
    expect(listMock).toHaveBeenNthCalledWith(2, {
      source_artifact: "operator/mainsite-posteditor",
      limit: 30,
    });
    expect(listMock).toHaveBeenNthCalledWith(3, {
      source_artifact: "operator/mainsite-posteditor",
      limit: 30,
      cursor: "new-cursor",
    });
  });

  it("does not retain the previous cursor if the new filtered query fails", async () => {
    listMock
      .mockResolvedValueOnce({ items: [], next_cursor: "old-cursor", total: 31 })
      .mockRejectedValueOnce(new Error("inventory unavailable"));

    render(<LinkIntegrityPanel recentRecords={[]} />);
    await screen.findByRole("button", { name: /Carregar mais \(0 de 31\)/ });
    fireEvent.change(screen.getByRole("combobox", { name: "Filtrar por origem do link" }), {
      target: { value: "operator/mainsite-posteditor" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Aplicar filtros" }));

    await screen.findByText("A leitura do inventário falhou. Nenhuma escrita foi tentada.");
    expect(screen.queryByRole("button", { name: /Carregar mais/ })).not.toBeInTheDocument();
    expect(listMock).toHaveBeenCalledTimes(2);
  });

  it.each([
    ["classification", { classification: "verified_supports_claim" }],
    ["cross-review", { cross_review_status: "accepted" }],
    ["pending review", { cross_review_status: "not_needed" }],
    ["source", { source_artifact: "operator/prompt-protocol" }],
    ["query", { review_note: null }],
  ] as const)("keeps new rows under the applied %s filter", async (filter, difference) => {
    listMock.mockResolvedValue({ items: [], next_cursor: "filtered-cursor", total: 0 });
    const view = render(<LinkIntegrityPanel recentRecords={[]} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Aplicar filtros" })).toBeEnabled(),
    );
    if (filter === "classification") {
      fireEvent.change(screen.getByRole("combobox", { name: "Filtrar por classificação" }), {
        target: { value: "not_found" },
      });
    } else if (filter === "cross-review") {
      fireEvent.change(
        screen.getByRole("combobox", { name: "Filtrar por estado de cross-review" }),
        { target: { value: "pending" } },
      );
    } else if (filter === "pending review") {
      fireEvent.click(screen.getByRole("checkbox", { name: "Somente pendentes de revisão" }));
    } else if (filter === "source") {
      fireEvent.change(screen.getByRole("combobox", { name: "Filtrar por origem do link" }), {
        target: { value: "operator/mainsite-posteditor" },
      });
    } else {
      fireEvent.change(screen.getByRole("searchbox", { name: "Buscar no inventário de links" }), {
        target: { value: " needle " },
      });
    }
    fireEvent.click(screen.getByRole("button", { name: "Aplicar filtros" }));
    await waitFor(() => expect(listMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Aplicar filtros" })).toBeEnabled(),
    );
    view.rerender(
      <LinkIntegrityPanel
        recentRecords={[
          linkRecord(),
          linkRecord({ link_id: "different-link", anchor_text: "different record", ...difference }),
        ]}
      />,
    );
    const inventory = within(screen.getByLabelText("Inventário de links auditados"));
    expect(inventory.getByRole("button", { name: /matching record/ })).toBeInTheDocument();
    expect(inventory.queryByRole("button", { name: /different record/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Carregar mais (1 de 1)" })).toBeInTheDocument();
  });

  it("removes a previously matching row when a new audit changes its classification", async () => {
    listMock.mockResolvedValue({ items: [], next_cursor: null, total: 0 });
    const view = render(<LinkIntegrityPanel recentRecords={[]} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Aplicar filtros" })).toBeEnabled(),
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Filtrar por classificação" }), {
      target: { value: "not_found" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Aplicar filtros" }));
    await waitFor(() => expect(listMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Aplicar filtros" })).toBeEnabled(),
    );
    view.rerender(<LinkIntegrityPanel recentRecords={[linkRecord()]} />);
    const inventory = within(screen.getByLabelText("Inventário de links auditados"));
    expect(inventory.getByRole("button", { name: /matching record/ })).toBeInTheDocument();
    view.rerender(
      <LinkIntegrityPanel
        recentRecords={[linkRecord({ classification: "verified_supports_claim" })]}
      />,
    );
    expect(inventory.queryByRole("button", { name: /matching record/ })).not.toBeInTheDocument();
    expect(screen.getByText("Nenhum link no filtro atual.")).toBeInTheDocument();
  });
});
