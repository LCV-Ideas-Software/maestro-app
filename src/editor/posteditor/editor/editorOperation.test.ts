import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";
import {
  applyEditorOperationResult,
  captureEditorOperationTarget,
  withEditorReadOnly,
} from "./editorOperation";

const editors: Editor[] = [];
afterEach(() => {
  for (const editor of editors.splice(0)) if (!editor.isDestroyed) editor.destroy();
});

function createEditor() {
  const editor = new Editor({ extensions: [StarterKit], content: "<p>alpha beta alpha</p>" });
  editors.push(editor);
  return editor;
}

describe("delayed editor operations", () => {
  it("replaces the requested range even when the selection moves before the response", () => {
    const editor = createEditor();
    editor.commands.setTextSelection({ from: 1, to: 6 });
    const target = captureEditorOperationTarget(editor);
    editor.commands.setTextSelection({ from: 7, to: 11 });
    expect(applyEditorOperationResult(editor, target, "UPDATED ALPHA")).toBe(true);
    expect(editor.getText()).toBe("UPDATED ALPHA beta alpha");
  });

  it.each([false, true])(
    "preserves newer edits instead of applying a stale response (whole document: %s)",
    (wholeDocument) => {
      const editor = createEditor();
      editor.commands.setTextSelection(wholeDocument ? 1 : { from: 1, to: 6 });
      const target = captureEditorOperationTarget(editor);
      editor.commands.insertContentAt(1, "NEW ");
      expect(() => applyEditorOperationResult(editor, target, "replacement")).toThrow(
        /preservadas/,
      );
      expect(editor.getText()).toBe("NEW alpha beta alpha");
    },
  );

  it("locks native editing until a deferred operation completes and restores it", async () => {
    const editor = createEditor();
    let finish!: () => void;
    const result = withEditorReadOnly(
      editor,
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    expect(editor.isEditable).toBe(false);
    expect(editor.view.dom.getAttribute("contenteditable")).toBe("false");
    finish();
    await result;
    expect(editor.isEditable).toBe(true);
  });

  it("restores editing after an operation rejects", async () => {
    const editor = createEditor();
    await expect(
      withEditorReadOnly(editor, async () => {
        throw new Error("offline");
      }),
    ).rejects.toThrow("offline");
    expect(editor.isEditable).toBe(true);
  });

  it("preserves an existing read-only state", async () => {
    const editor = createEditor();
    editor.setEditable(false);
    await withEditorReadOnly(editor, async () => undefined);
    expect(editor.isEditable).toBe(false);
  });

  it("settles safely if the editor is destroyed before the response", async () => {
    const editor = createEditor();
    const target = captureEditorOperationTarget(editor);
    let finish!: () => void;
    const result = withEditorReadOnly(
      editor,
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    editor.destroy();
    finish();
    await expect(result).resolves.toBeUndefined();
    expect(() => applyEditorOperationResult(editor, target, "late response")).toThrow(/fechado/);
  });
});
