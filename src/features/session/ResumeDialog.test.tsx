import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ResumeDialog } from "./ResumeDialog";

beforeEach(() => {
  // jsdom does not implement the browser's top layer; test only native API lifecycle.
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
    configurable: true,
    value: vi.fn(function (this: HTMLDialogElement) {
      this.open = true;
    }),
  });
  Object.defineProperty(HTMLDialogElement.prototype, "close", {
    configurable: true,
    value: vi.fn(function (this: HTMLDialogElement) {
      this.open = false;
    }),
  });
});

afterEach(() => cleanup());

function renderDialog(onClose = vi.fn()) {
  return render(
    <ResumeDialog
      candidates={[]}
      hasLoadedProtocol={false}
      protocol={{ name: "protocol.md", hash: "hash", lines: 1, size: 1 }}
      useLoadedProtocol={false}
      formatActivity={() => "now"}
      onClose={onClose}
      onChoose={vi.fn()}
      onUseLoadedProtocolChange={vi.fn()}
    />,
  );
}

describe("ResumeDialog native modal lifecycle", () => {
  it("opens through showModal and closes the native dialog on unmount", () => {
    const view = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "Retomar sessao" });
    expect(dialog.tagName).toBe("DIALOG");
    expect(HTMLDialogElement.prototype.showModal).toHaveBeenCalledOnce();
    view.unmount();
    expect(HTMLDialogElement.prototype.close).toHaveBeenCalledOnce();
  });

  it("maps native Escape cancellation to the existing close callback", () => {
    const close = vi.fn();
    renderDialog(close);
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(close).toHaveBeenCalledOnce();
  });
});
