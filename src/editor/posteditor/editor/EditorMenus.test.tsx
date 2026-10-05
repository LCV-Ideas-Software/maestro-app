import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorBubbleMenu } from "./BubbleMenu";
import { EditorFloatingMenu } from "./FloatingMenu";
import { SelectMediaButton } from "./NodeViews";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];
const rangeBounds = Object.getOwnPropertyDescriptor(Range.prototype, "getBoundingClientRect");
const rangeRects = Object.getOwnPropertyDescriptor(Range.prototype, "getClientRects");

beforeEach(() => {
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => new DOMRect(40, 100, 80, 20),
  });
  Object.defineProperty(Range.prototype, "getClientRects", {
    configurable: true,
    value: () => [new DOMRect(40, 100, 80, 20)],
  });
});

afterEach(() => {
  cleanup();
  for (const editor of editors.splice(0)) editor.destroy();
  for (const element of elements.splice(0)) element.remove();
  if (rangeBounds) Object.defineProperty(Range.prototype, "getBoundingClientRect", rangeBounds);
  else Reflect.deleteProperty(Range.prototype, "getBoundingClientRect");
  if (rangeRects) Object.defineProperty(Range.prototype, "getClientRects", rangeRects);
  else Reflect.deleteProperty(Range.prototype, "getClientRects");
  vi.restoreAllMocks();
});

function createEditor(content: string) {
  const element = document.createElement("div");
  document.body.appendChild(element);
  elements.push(element);
  const editor = new Editor({ element, extensions: [StarterKit], content });
  editors.push(editor);
  vi.spyOn(editor.view, "coordsAtPos").mockReturnValue({
    top: 100,
    bottom: 120,
    left: 40,
    right: 120,
  });
  return editor;
}

describe("native editor menu activation", () => {
  it("executes a BubbleMenu command through the keyboard-generated click", () => {
    const editor = createEditor("<p>alpha beta</p>");
    render(<EditorBubbleMenu editor={editor} />);
    act(() => editor.commands.setTextSelection({ from: 1, to: 6 }));
    const button = screen.getByTitle("Negrito (Ctrl+B)");
    fireEvent.blur(editor.view.dom, { relatedTarget: button });
    expect(button).toBeInTheDocument();
    fireEvent.click(button, { detail: 0 });
    expect(editor.getHTML()).toBe("<p><strong>alpha</strong> beta</p>");
  });

  it("preserves selection on mousedown and executes exactly once on click", () => {
    const editor = createEditor("<p>alpha beta</p>");
    render(<EditorBubbleMenu editor={editor} />);
    act(() => editor.commands.setTextSelection({ from: 1, to: 6 }));
    const button = screen.getByTitle("Negrito (Ctrl+B)");
    fireEvent.mouseDown(button);
    expect(editor.getHTML()).toBe("<p>alpha beta</p>");
    fireEvent.click(button, { detail: 1 });
    expect(editor.getHTML()).toBe("<p><strong>alpha</strong> beta</p>");
  });

  it("executes a FloatingMenu command through click without requiring mousedown", () => {
    const editor = createEditor("<p></p>");
    render(<EditorFloatingMenu editor={editor} onInsertTable={vi.fn()} />);
    act(() => editor.emit("selectionUpdate", { editor, transaction: editor.state.tr }));
    const button = screen.getByTitle("Título 1");
    fireEvent.blur(editor.view.dom, { relatedTarget: button });
    expect(button).toBeInTheDocument();
    fireEvent.click(button, { detail: 0 });
    expect(editor.isActive("heading", { level: 1 })).toBe(true);
  });

  it("lets keyboard activation select media", () => {
    const select = vi.fn();
    render(<SelectMediaButton onSelect={select} />);
    fireEvent.click(screen.getByRole("button", { name: "Selecionar mídia" }), { detail: 0 });
    expect(select).toHaveBeenCalledOnce();
  });
});
