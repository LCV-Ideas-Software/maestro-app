import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";
import { SearchReplacePanel } from "./SearchReplace";
import { SearchReplaceExtension } from "./searchReplaceCore";

const editors: Editor[] = [];

afterEach(() => {
  cleanup();
  for (const editor of editors.splice(0)) editor.destroy();
});

function openPanel(content: string, replacement: string) {
  const editor = new Editor({ extensions: [StarterKit, SearchReplaceExtension], content });
  editors.push(editor);
  render(<SearchReplacePanel editor={editor} />);
  act(() => document.dispatchEvent(new CustomEvent("tiptap:search-toggle")));
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "alpha" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Texto de substituição" }), {
    target: { value: replacement },
  });
  return editor;
}

describe("SearchReplacePanel", () => {
  it("deletes all matches when the replacement is empty without creating an empty text node", () => {
    const editor = openPanel("<p>alpha beta alpha</p><p>alpha</p>", "");
    fireEvent.click(screen.getByRole("button", { name: "Tudo" }));
    expect(editor.getText()).toBe(" beta \n\n");
    expect(editor.getHTML()).toBe("<p> beta </p><p></p>");
  });

  it.each(["Substituir", "Tudo"])(
    "treats HTML-looking replacement as literal text with %s",
    (name) => {
      const editor = openPanel("<p>alpha beta alpha</p>", "<strong>texto</strong>");
      fireEvent.click(screen.getByRole("button", { name }));
      expect(editor.getHTML()).toContain("&lt;strong&gt;texto&lt;/strong&gt;");
      expect(editor.getHTML()).not.toContain("<strong>");
      expect(editor.getText()).toBe(
        name === "Tudo"
          ? "<strong>texto</strong> beta <strong>texto</strong>"
          : "<strong>texto</strong> beta alpha",
      );
    },
  );

  it("keeps subsequent match positions correct when replacements grow and include Unicode", () => {
    const editor = openPanel("<p>alpha beta alpha</p><p>alpha</p>", "🌎 & revisão");
    fireEvent.click(screen.getByRole("button", { name: "Tudo" }));
    expect(editor.getText()).toBe("🌎 & revisão beta 🌎 & revisão\n\n🌎 & revisão");
  });
});
