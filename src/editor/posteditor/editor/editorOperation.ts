import type { Editor } from "@tiptap/core";
import type { Node as ProseMirrorNode } from "prosemirror-model";

export type EditorOperationTarget = {
  document: ProseMirrorNode;
  from: number;
  to: number;
  wholeDocument: boolean;
};

export function captureEditorOperationTarget(editor: Editor): EditorOperationTarget {
  const { from, to, empty } = editor.state.selection;
  return { document: editor.state.doc, from, to, wholeDocument: empty };
}

export function applyEditorOperationResult(
  editor: Editor,
  target: EditorOperationTarget,
  html: string,
): boolean {
  if (editor.isDestroyed) throw new Error("O editor foi fechado antes de concluir a operação.");
  if (!editor.state.doc.eq(target.document)) {
    throw new Error(
      "O conteúdo mudou durante a operação. As alterações foram preservadas; tente novamente.",
    );
  }
  return target.wholeDocument
    ? editor.commands.setContent(html)
    : editor.commands.insertContentAt({ from: target.from, to: target.to }, html);
}

export async function withEditorReadOnly<T>(
  editor: Editor,
  operation: () => Promise<T>,
): Promise<T> {
  const wasEditable = editor.isEditable;
  editor.setEditable(false);
  try {
    return await operation();
  } finally {
    if (!editor.isDestroyed) editor.setEditable(wasEditable);
  }
}
