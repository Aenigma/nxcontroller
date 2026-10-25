// Editor awareness: rewrites text so that what ends up in the target editor matches
// the original, given what that editor does on its own when you type. Works on
// prepare()d text; the result may contain SPECIAL keys.

import { SPECIAL } from "./protocol";

export const BACKSPACE = SPECIAL.BACKSPACE;

export type EditorMode = "normal" | "smilebasic" | "strip";

export const EDITOR_MODES: readonly { id: EditorMode; label: string; hint: string }[] = [
  { id: "normal", label: "Normal", hint: "Types the text as is." },
  { id: "smilebasic", label: "SmileBASIC", hint: "For SmileBASIC 4's editor. Start on an empty line." },
  { id: "strip", label: "Strip indent", hint: "For editors that indent for you, like VS Code." },
];

export interface EditorOptions {
  /** SmileBASIC: tab stops every this many columns. */
  tabWidth?: number;
}

/**
 * SmileBASIC: lines that can end with keyword suggestions open (after "END", say). Enter
 * would pick a suggestion instead of starting a new line; End closes them and types nothing.
 */
const ENDS_IN_WORD = /[A-Za-z0-9_$#%]$/;

/** What to type so the editor ends up holding `text`. */
export function adaptForEditor(text: string, mode: EditorMode, { tabWidth = 1 }: EditorOptions = {}): string {
  switch (mode) {
    case "normal":
      return text;
    case "strip":
      return text.split("\n").map((line) => line.replace(/^[ \t]+/, "")).join("\n");
    case "smilebasic": {
      const lines = text.split("\n");
      let carried = 0; // spaces the editor puts at the start of the next line
      return lines.map((raw, i) => {
        const line = expandTabs(raw, Math.max(1, tabWidth));
        const indent = line.length - line.trimStart().length;
        const fix = indent >= carried ? " ".repeat(indent - carried) : BACKSPACE.repeat(carried - indent);
        carried = indent;
        const last = i === lines.length - 1;
        return fix + line.slice(indent) + (!last && ENDS_IN_WORD.test(line) ? SPECIAL.END : "");
      }).join("\n");
    }
  }
}

/** Replaces tabs with spaces up to the next tab stop. */
export function expandTabs(line: string, width: number): string {
  let out = "";
  for (const ch of line) out += ch === "\t" ? " ".repeat(width - (out.length % width)) : ch;
  return out;
}
