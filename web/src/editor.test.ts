import { describe, expect, it } from "vitest";
import { BACKSPACE as BS, adaptForEditor, expandTabs, smilebasicChecksum, smilebasicErase, smilebasicVerify } from "./editor";
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

describe("smilebasicChecksum", () => {
  /** What the checker commands compute, line by line as SmileBASIC would. */
  function checker(editorLines: string[]) {
    let s = editorLines.join("\n");                      // PRGGET$() includes each line's break
    while (s.endsWith("\n")) s = s.slice(0, -1);         // WHILE RIGHT$(S$,1)==CHR$(10) ...
    let h = 0;
    for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) % 999983;
    return { chars: s.length, checksum: h };
  }

  it("matches what the checker reads back after typing in SmileBASIC mode", () => {
    const text = PROGRAM + "\tPRINT \"TAB\"\n\n\n";
    const editor = smilebasicEditor(adaptForEditor(text, "smilebasic")).split("\n");
    expect(checker(editor)).toEqual(smilebasicChecksum(text));
  });

  it("changes when a line break goes missing", () => {
    expect(smilebasicChecksum("END\nCOMMON DEF F").checksum).not.toBe(smilebasicChecksum("ENDCOMMON DEF F").checksum);
  });

  it("is a small, hand-checkable hash", () => {
    expect(smilebasicChecksum("AB\n\n")).toEqual({ chars: 2, checksum: 65 * 31 + 66 });
  });

  it("types each command with End before its Enter", () => {
    for (const line of [...smilebasicVerify(3), smilebasicErase(3)]) {
      expect(adaptForEditor(line + "\n", "smilebasic")).toBe(line + SPECIAL.END + "\n");
      expect(line.length).toBeLessThan(100);
    }
  });

  it("works on the chosen slot", () => {
    expect(smilebasicVerify(2)[0]).toContain("PRGEDIT 2:");
    expect(smilebasicVerify(2)[0]).toContain("PRGSIZE(2)");
    expect(smilebasicErase(2)).toBe("PRGEDIT 2:PRGDEL -1");
  });
});

describe("expandTabs", () => {
  it("expands to the next tab stop", () => {
    expect(expandTabs("\ta\tbc\t", 4)).toBe("    a   bc  ");
    expect(expandTabs(" \t", 2)).toBe("  ");
  });
});
