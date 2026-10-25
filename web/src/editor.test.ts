import { describe, expect, it } from "vitest";
import { BACKSPACE as BS, adaptForEditor, expandTabs } from "./editor";
import { SPECIAL } from "./protocol";

/** Plays keystrokes into a SmileBASIC-style editor: Enter copies the previous line's leading spaces. */
function smilebasicEditor(keys: string): string {
  const lines = [""];
  for (const k of keys) {
    const cur = lines.length - 1;
    if (k === "\n") lines.push(" ".repeat(lines[cur]!.length - lines[cur]!.trimStart().length));
    else if (k === BS) lines[cur] = lines[cur]!.slice(0, -1);
    else if (k === "\t") throw new Error("the editor has no tabs");
    else if (k === SPECIAL.END) continue; // already at the end of the line
    else lines[cur] += k;
  }
  return lines.join("\n");
}

const PROGRAM = [
  "DEF FOO X",
  "  IF X THEN",
  "    PRINT 1",
  "",
  "  ENDIF",
  "END",
  "",
].join("\n");

describe("adaptForEditor", () => {
  it("leaves text alone in normal mode", () => {
    expect(adaptForEditor("a\n\tb\n  c", "normal")).toBe("a\n\tb\n  c");
  });

  it("strips leading spaces and tabs, keeping the rest of each line", () => {
    expect(adaptForEditor("a\n\tb  c\n    d\t\n  \n", "strip")).toBe("a\nb  c\nd\t\n\n");
  });

  it("types only the change in indentation for SmileBASIC", () => {
    const keys = adaptForEditor(PROGRAM, "smilebasic");
    const E = SPECIAL.END;
    expect(keys).toBe([
      "DEF FOO X" + E,
      "  IF X THEN" + E,
      "  PRINT 1" + E,       // 2 more on top of the 2 carried over
      BS.repeat(4),          // the blank line has none
      "  ENDIF" + E,
      BS.repeat(2) + "END" + E,
      "",
    ].join("\n"));
    expect(smilebasicEditor(keys)).toBe(PROGRAM);
  });

  it("reproduces indented text exactly in a SmileBASIC-style editor", () => {
    const text = "  a\n      b\n  \n\tc\n \t d\n\n    e   \nf";
    const expected = "  a\n      b\n  \n    c\n     d\n\n    e   \nf";
    expect(smilebasicEditor(adaptForEditor(text, "smilebasic", { tabWidth: 4 }))).toBe(expected);
    expect(smilebasicEditor(adaptForEditor(text, "smilebasic", { tabWidth: 2 }))).toBe(expected.replace("    c", "  c").replace("     d", "   d"));
    // Default: a tab is one space.
    expect(smilebasicEditor(adaptForEditor(text, "smilebasic"))).toBe(expected.replace("    c", " c").replace("     d", "   d"));
  });

  it("presses a key before Enter on lines ending in a word, to close keyword suggestions", () => {
    const text = "END\nX=1\nA$\nPRINT \"HI\"\n\nEND";
    const typed = adaptForEditor(text, "smilebasic"), E = SPECIAL.END;
    // Not after a quote, a blank line, or the last line (no Enter follows it).
    expect(typed).toBe(`END${E}\nX=1${E}\nA$${E}\nPRINT "HI"\n\nEND`);
    expect(smilebasicEditor(typed)).toBe(text);
  });

  it("never sends Tab in SmileBASIC mode", () => {
    expect(adaptForEditor("\tx\ty\n", "smilebasic")).toBe(` x y${SPECIAL.END}\n${BS}`); // the empty last line undoes the carried space
    expect(adaptForEditor("\tx\ty", "smilebasic", { tabWidth: 4 })).toBe("    x   y");
  });
});

describe("expandTabs", () => {
  it("expands to the next tab stop", () => {
    expect(expandTabs("\ta\tbc\t", 4)).toBe("    a   bc  ");
    expect(expandTabs(" \t", 2)).toBe("  ");
  });
});
