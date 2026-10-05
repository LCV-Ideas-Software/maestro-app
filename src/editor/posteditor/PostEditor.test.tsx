import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Editor } from "@tiptap/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { buildFinalContentExport } from "./editor/exportFinalContent";
import PostEditor, { type PostEditorProps } from "./PostEditor";

const editorBox = vi.hoisted(() => ({ current: null as Editor | null }));

vi.mock("@tiptap/react", async (importOriginal) => {
  const module = await importOriginal<typeof import("@tiptap/react")>();
  return {
    ...module,
    useEditor: (...args: Parameters<typeof module.useEditor>) => {
      const editor = module.useEditor(...args);
      editorBox.current = editor;
      return editor;
    },
  };
});
vi.mock("./editor/extensions", async () => {
  const { default: StarterKit } = await import("@tiptap/starter-kit");
  const { CharacterCount } = await import("@tiptap/extension-character-count");
  return {
    EDITORIAL_MENTION_BASE_ITEMS: [],
    buildTiptapExtensions: () => [StarterKit, CharacterCount],
  };
});
vi.mock("@tiptap/extension-drag-handle-react", () => ({ DragHandle: () => null }));
vi.mock("./editor/BubbleMenu", () => ({ EditorBubbleMenu: () => null }));
vi.mock("./editor/FloatingMenu", () => ({ EditorFloatingMenu: () => null }));
vi.mock("./editor/exportFinalContent", async (importOriginal) => {
  const module = await importOriginal<typeof import("./editor/exportFinalContent")>();
  return {
    ...module,
    buildFinalContentExport: vi.fn(module.buildFinalContentExport),
    downloadExportArtifact: vi.fn(),
    openFinalContentPrintDialog: vi.fn(),
  };
});

const evidence = {
  provider: "chatgpt" as const,
  id: "web-evidence-1",
  source_url: "https://chatgpt.com/share/editorial",
  sha256: "b".repeat(64),
  retrieved_at: "2026-10-05T12:00:00.000Z",
};

function renderEditor(props: Partial<PostEditorProps> = {}) {
  return render(
    <PostEditor
      editingPostId={null}
      initialTitle="Draft"
      initialAuthor="Author"
      initialContent="<p>alpha beta alpha</p>"
      savingPost={false}
      showNotification={vi.fn()}
      onSave={vi.fn().mockResolvedValue(true)}
      onClose={vi.fn()}
      {...props}
    />,
  );
}

afterEach(() => {
  cleanup();
  editorBox.current = null;
  vi.clearAllMocks();
});

describe("PostEditor desktop custody", () => {
  it("exposes unavailable native capabilities as disabled controls without calling an admin HTTP endpoint", async () => {
    renderEditor();
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    for (const control of screen.getAllByTitle(
      "Transformação por IA indisponível nesta sessão desktop",
    )) {
      expect(control).toBeDisabled();
    }
    expect(
      screen.getByTitle("Upload indisponível nesta sessão desktop; use Imagem por URL"),
    ).toBeDisabled();
    expect(screen.getByTitle("Imagem por URL / Google Drive")).toBeEnabled();
    act(() => document.dispatchEvent(new CustomEvent("tiptap:slash-ai")));
    expect(screen.getByRole("status")).toHaveTextContent(
      "não está disponível nesta sessão desktop",
    );
    expect(document.querySelector(".ai-freeform-popover")).not.toBeInTheDocument();
  });

  it("locks document editing while a save is pending and restores it when saving rejects", async () => {
    let reject!: (error: Error) => void;
    const save = vi.fn(
      () =>
        new Promise<boolean>((_resolve, rejectPromise) => {
          reject = rejectPromise;
        }),
    );
    renderEditor({ onSave: save });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Criar post" }));
    expect(editorBox.current?.isEditable).toBe(false);
    expect(screen.getByRole("button", { name: "Criar post" })).toBeDisabled();
    expect(document.querySelector(".tiptap-toolbar")).toHaveAttribute("inert");
    await act(async () => reject(new Error("disk unavailable")));
    expect(editorBox.current?.isEditable).toBe(true);
    expect(screen.getByRole("button", { name: "Criar post" })).toBeEnabled();
    expect(save).toHaveBeenCalledOnce();
  });

  it("applies a deferred native AI response to its original selection", async () => {
    let resolve!: (text: string) => void;
    const transform = vi.fn(
      () =>
        new Promise<string>((resolvePromise) => {
          resolve = resolvePromise;
        }),
    );
    renderEditor({ onTransformText: transform });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    act(() => editorBox.current?.commands.setTextSelection({ from: 1, to: 6 }));
    fireEvent.change(screen.getByTitle("Inteligência Artificial"), {
      target: { value: "grammar" },
    });
    expect(transform).toHaveBeenCalledWith({ action: "grammar", text: "alpha" });
    expect(editorBox.current?.isEditable).toBe(false);
    act(() => editorBox.current?.commands.setTextSelection({ from: 7, to: 11 }));
    await act(async () => resolve("UPDATED ALPHA"));
    expect(editorBox.current?.getText()).toBe("UPDATED ALPHA beta alpha");
    expect(editorBox.current?.isEditable).toBe(true);
  });

  it("preserves newer document content and shows a visible error for a stale AI response", async () => {
    let resolve!: (text: string) => void;
    renderEditor({
      onTransformText: () =>
        new Promise<string>((resolvePromise) => {
          resolve = resolvePromise;
        }),
    });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    act(() => editorBox.current?.commands.setTextSelection({ from: 1, to: 6 }));
    fireEvent.change(screen.getByTitle("Inteligência Artificial"), {
      target: { value: "grammar" },
    });
    act(() => editorBox.current?.commands.insertContentAt(1, "NEW "));
    await act(async () => resolve("replacement"));
    expect(editorBox.current?.getText()).toBe("NEW alpha beta alpha");
    expect(screen.getByRole("status")).toHaveTextContent("As alterações foram preservadas");
  });

  it("passes durable provenance to save and exports it after reopening without putting it in article HTML", async () => {
    const save = vi.fn().mockResolvedValue(true);
    const view = renderEditor({ initialSharedChatEvidence: [evidence], onSave: save });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Criar post" }));
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0]?.[7]).toEqual([evidence]);
    expect(save.mock.calls[0]?.[2]).not.toContain(evidence.source_url);
    view.unmount();
    renderEditor({ initialSharedChatEvidence: [evidence] });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "HTML MainSite" }));
    const call = vi.mocked(buildFinalContentExport).mock.calls[0];
    expect(call?.[0].evidence).toEqual([evidence]);
    expect(call?.[0].html).not.toContain(evidence.source_url);
  });
});
