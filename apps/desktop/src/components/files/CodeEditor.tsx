import { useEffect, useRef } from "react";
import { indentWithTab } from "@codemirror/commands";
import {
  HighlightStyle,
  LanguageDescription,
  syntaxHighlighting,
} from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { basicSetup } from "codemirror";

import { useTheme } from "@/hooks/useTheme";
import { fileName } from "@/lib/diff";

/// Every colour is a token, so one style serves every palette and both modes.
/// The mapping follows the meanings the rest of the app already gives those
/// accents: strings green like added lines, keywords the issue violet, calls
/// the mention blue, literals the command yellow.
const HIGHLIGHT = HighlightStyle.define([
  { tag: tags.comment, color: "var(--muted-foreground)" },
  { tag: [tags.keyword, tags.modifier, tags.operatorKeyword], color: "var(--accent-issue)" },
  { tag: [tags.string, tags.special(tags.string), tags.regexp], color: "var(--accent-add)" },
  { tag: [tags.number, tags.bool, tags.atom, tags.null], color: "var(--accent-command)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--accent-mention)" },
  { tag: [tags.typeName, tags.className, tags.namespace], color: "var(--accent-session)" },
  { tag: [tags.tagName], color: "var(--accent-issue)" },
  { tag: [tags.attributeName, tags.definition(tags.variableName)], color: "var(--accent-command)" },
  { tag: [tags.operator, tags.punctuation, tags.separator], color: "var(--muted-foreground)" },
  { tag: tags.heading, fontWeight: "600" },
  { tag: tags.strong, fontWeight: "600" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.link, textDecoration: "underline" },
  { tag: tags.invalid, color: "var(--destructive)" },
]);

function theme(dark: boolean) {
  return EditorView.theme(
    {
      "&": { height: "100%", backgroundColor: "transparent", color: "var(--foreground)" },
      "&.cm-focused": { outline: "none" },
      ".cm-scroller": {
        fontFamily: "var(--font-mono)",
        fontSize: "inherit",
        lineHeight: "1.6",
      },
      ".cm-content": { caretColor: "var(--foreground)", padding: "8px 0" },
      ".cm-gutters": {
        backgroundColor: "transparent",
        color: "var(--muted-foreground)",
        border: "none",
        paddingLeft: "4px",
      },
      ".cm-activeLine": { backgroundColor: "var(--veil-selected)" },
      ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--foreground)" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground": {
        backgroundColor: "color-mix(in oklab, var(--primary) 25%, transparent)",
      },
      ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": {
        backgroundColor: "var(--veil-strong)",
        outline: "none",
      },
      ".cm-searchMatch": { backgroundColor: "color-mix(in oklab, var(--accent-command) 30%, transparent)" },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: "color-mix(in oklab, var(--accent-command) 55%, transparent)",
      },
      ".cm-panels": {
        backgroundColor: "var(--popover)",
        color: "var(--popover-foreground)",
        borderColor: "var(--border)",
      },
      ".cm-panel input, .cm-panel button": {
        borderRadius: "var(--radius-md)",
        border: "1px solid var(--input)",
        background: "transparent",
        color: "inherit",
      },
      ".cm-tooltip": {
        backgroundColor: "var(--popover)",
        color: "var(--popover-foreground)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-md)",
      },
      ".cm-foldPlaceholder": {
        backgroundColor: "var(--muted)",
        color: "var(--muted-foreground)",
        border: "none",
      },
    },
    { dark },
  );
}

export default function CodeEditor({
  path,
  value,
  readOnly,
  line,
  reveal,
  onChange,
  onBlur,
}: {
  path: string;
  /// What the buffer should hold. Set from outside only when the file was
  /// reloaded or saved by something other than typing; keystrokes report up
  /// through `onChange` and the view keeps its own copy.
  value: string;
  readOnly: boolean;
  line?: number;
  reveal: number;
  onChange: (text: string) => void;
  onBlur: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const editable = useRef(new Compartment());
  const palette = useRef(new Compartment());
  const { resolvedMode } = useTheme();

  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onBlurRef = useRef(onBlur);
  onBlurRef.current = onBlur;

  useEffect(() => {
    if (!host.current) return;
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        keymap.of([indentWithTab]),
        syntaxHighlighting(HIGHLIGHT),
        palette.current.of(theme(resolvedMode === "dark")),
        language.current.of([]),
        editable.current.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChangeRef.current(update.state.doc.toString());
        }),
        EditorView.domEventHandlers({
          blur: () => {
            onBlurRef.current();
          },
        }),
      ],
    });
    const created = new EditorView({ state, parent: host.current });
    view.current = created;

    let cancelled = false;
    const description = LanguageDescription.matchFilename(languages, fileName(path));
    void description?.load().then((support) => {
      if (cancelled) return;
      created.dispatch({ effects: language.current.reconfigure(support) });
    });

    return () => {
      cancelled = true;
      created.destroy();
      view.current = null;
    };
    // The view is built once per path; `value` and `readOnly` are pushed in
    // through the effects below rather than rebuilding, which would lose the
    // caret and the undo history.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  useEffect(() => {
    const current = view.current;
    if (!current) return;
    const held = current.state.doc.toString();
    if (held === value) return;
    current.dispatch({ changes: { from: 0, to: held.length, insert: value } });
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({
      effects: editable.current.reconfigure([
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
      ]),
    });
  }, [readOnly]);

  useEffect(() => {
    view.current?.dispatch({
      effects: palette.current.reconfigure(theme(resolvedMode === "dark")),
    });
  }, [resolvedMode]);

  useEffect(() => {
    const current = view.current;
    if (!current || !line) return;
    if (line > current.state.doc.lines) return;
    const pos = current.state.doc.line(line).from;
    current.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
    });
  }, [line, reveal]);

  return (
    <div
      ref={host}
      data-file-editor
      className="min-h-0 min-w-0 flex-1 overflow-hidden text-code"
    />
  );
}
