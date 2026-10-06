import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Editor } from "@tiptap/core";
import { lazy, StrictMode, Suspense } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "../../components/ErrorBoundary";
import {
  buildFinalContentExport,
  downloadExportArtifact,
  openFinalContentPrintDialog,
} from "./editor/exportFinalContent";
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
  it("opens and saves a new post after delayed lazy loading through the complete production editor", async () => {
    const save = vi.fn().mockResolvedValue(true);
    let resolveEditor!: (module: { default: typeof PostEditor }) => void;
    const LazyPostEditor = lazy(
      () =>
        new Promise<{ default: typeof PostEditor }>((resolve) => {
          resolveEditor = resolve;
        }),
    );
    render(
      <StrictMode>
        <ErrorBoundary>
          <p>Workbench remains available</p>
          <Suspense fallback={<p>Loading editorial screen</p>}>
            <LazyPostEditor
              editingPostId={null}
              initialTitle="New article"
              initialAuthor="Author"
              initialContent="<h1>Article in preparation</h1><p>Initial editorial text.</p>"
              savingPost={false}
              showNotification={vi.fn()}
              onSave={save}
              onClose={vi.fn()}
            />
          </Suspense>
        </ErrorBoundary>
      </StrictMode>,
    );
    expect(screen.getByText("Loading editorial screen")).toBeInTheDocument();
    expect(editorBox.current).toBeNull();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
      resolveEditor({ default: PostEditor });
    });
    const create = await screen.findByRole("button", { name: "Criar post" });
    expect(screen.getByText("Workbench remains available")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(editorBox.current?.isEditable).toBe(true);
    expect(editorBox.current?.getText()).toContain("Initial editorial text.");
    fireEvent.click(create);
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0]?.[0]).toBe("New article");
    expect(save.mock.calls[0]?.[1]).toBe("Author");
    expect(save.mock.calls[0]?.[2]).toContain("Article in preparation");
    expect(save.mock.calls[0]?.[2]).toContain("Initial editorial text.");
    expect(screen.getByText("Workbench remains available")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

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

  it("reports a failed native PDF request before downloading provenance and preserves article editing", async () => {
    renderEditor({ initialSharedChatEvidence: [evidence] });
    await waitFor(() => expect(editorBox.current).not.toBeNull());
    let rejectPrint!: (error: Error) => void;
    vi.mocked(openFinalContentPrintDialog).mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectPrint = reject;
        }),
    );

    fireEvent.click(screen.getByRole("button", { name: "PDF" }));

    expect(downloadExportArtifact).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    await act(async () => rejectPrint(new Error("Native print unavailable")));
    expect(screen.getByRole("status")).toHaveTextContent("Native print unavailable");
    expect(editorBox.current?.isEditable).toBe(true);
    expect(editorBox.current?.getText()).toBe("alpha beta alpha");
    fireEvent.click(screen.getByRole("button", { name: "PDF" }));
    await waitFor(() => expect(downloadExportArtifact).toHaveBeenCalledOnce());
    expect(vi.mocked(downloadExportArtifact).mock.calls[0]?.[0].filename).toBe(
      "draft.provenance.json",
    );
    expect(screen.getByRole("status")).toHaveTextContent("Impressão solicitada");
  });
});
