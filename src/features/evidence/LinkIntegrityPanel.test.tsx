import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listLinkIntegrityRecords } from "../../services/evidence";
import { LinkIntegrityPanel } from "./LinkIntegrityPanel";

vi.mock("../../services/evidence", () => ({
  listLinkIntegrityRecords: vi.fn(),
  proposeLinkCorrections: vi.fn(),
  reviewLinkIntegrity: vi.fn(),
}));

const listMock = vi.mocked(listLinkIntegrityRecords);

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
});
