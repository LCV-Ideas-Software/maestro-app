import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { aiProviderRows, providerRateRows } from "../../constants";
import type { AiCredentialKey, ProviderRateKey } from "../../types";
import { AiProviderSettingsPanel } from "./AiProviderSettingsPanel";

afterEach(() => cleanup());

function panelProps(): ComponentProps<typeof AiProviderSettingsPanel> {
  return {
    aiConfigStatus: "Configuration ready",
    aiCredentials: Object.fromEntries(
      aiProviderRows.map((provider) => [provider.key, ""]),
    ) as Record<AiCredentialKey, string>,
    agyCliProjectId: "",
    isSaving: false,
    isVerifying: false,
    probeRows: [],
    providerInputRates: Object.fromEntries(
      providerRateRows.map((provider) => [provider.key, "1"]),
    ) as Record<ProviderRateKey, string>,
    providerOutputRates: Object.fromEntries(
      providerRateRows.map((provider) => [provider.key, "2"]),
    ) as Record<ProviderRateKey, string>,
    providerMode: "hybrid",
    onChooseProviderMode: vi.fn(),
    onCredentialChange: vi.fn(),
    onAgyCliProjectIdChange: vi.fn(),
    onInputRateChange: vi.fn(),
    onOutputRateChange: vi.fn(),
    onSave: vi.fn(),
    onVerify: vi.fn(),
  };
}

describe("provider configuration custody", () => {
  it.each(["isSaving", "isVerifying", "isBusy"] as const)(
    "prevents overlapping mode writes and input loss during %s",
    (busyFlag) => {
      const props = panelProps();
      const view = render(<AiProviderSettingsPanel {...props} {...{ [busyFlag]: true }} />);
      const modeControls = within(screen.getByLabelText("Modo dos provedores"));
      for (const button of modeControls.getAllByRole("button")) {
        expect(button).toBeDisabled();
        fireEvent.click(button);
      }
      expect(props.onChooseProviderMode).not.toHaveBeenCalled();
      const inputs = view.container.querySelectorAll("input");
      expect(inputs.length).toBe(19);
      for (const input of inputs) expect(input).toBeDisabled();
      view.rerender(<AiProviderSettingsPanel {...props} />);
      for (const input of inputs) expect(input).toBeEnabled();
      fireEvent.click(modeControls.getByRole("button", { name: "CLI" }));
      expect(props.onChooseProviderMode).toHaveBeenCalledExactlyOnceWith("cli");
    },
  );

  it("keeps the native project selection explicit and separate from credentials", () => {
    const props = panelProps();
    const view = render(<AiProviderSettingsPanel {...props} />);
    const projectInput = screen.getByLabelText("ID do projeto nativo do AGY CLI");
    expect(projectInput).toHaveValue("");
    expect(projectInput).toHaveAttribute("type", "text");
    fireEvent.change(projectInput, { target: { value: "operator-selected-native-project" } });
    expect(props.onAgyCliProjectIdChange).toHaveBeenCalledExactlyOnceWith(
      "operator-selected-native-project",
    );
    expect(props.onCredentialChange).not.toHaveBeenCalled();
    expect(props.onChooseProviderMode).not.toHaveBeenCalled();
    expect(props.onSave).not.toHaveBeenCalled();
    view.rerender(<AiProviderSettingsPanel {...props} agyCliProjectId="saved-native-project" />);
    expect(projectInput).toHaveValue("saved-native-project");
    expect(projectInput).toHaveAccessibleDescription(/bloqueia a execucao/);
  });

  it("preserves the configured CLI selection across explicit API mode changes", () => {
    const props = { ...panelProps(), agyCliProjectId: "saved-native-project" };
    const view = render(<AiProviderSettingsPanel {...props} providerMode="api" />);
    expect(screen.queryByLabelText("ID do projeto nativo do AGY CLI")).toBeNull();
    expect(props.onAgyCliProjectIdChange).not.toHaveBeenCalled();
    view.rerender(<AiProviderSettingsPanel {...props} providerMode="cli" />);
    expect(screen.getByLabelText("ID do projeto nativo do AGY CLI")).toHaveValue(
      "saved-native-project",
    );
  });
});
