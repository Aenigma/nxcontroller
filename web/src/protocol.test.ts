import { describe, expect, it } from "vitest";
import { FrameParser, KEYMAP, LCTRL, LSHIFT, MSG, SPECIAL, frame, parseHelloAck, prepare } from "./protocol";

describe("frame", () => {
  it("prefixes type and length", () => {
    expect([...frame(MSG.PING, [1, 0, 0, 0])]).toEqual([0x02, 4, 1, 0, 0, 0]);
    expect([...frame(MSG.RELEASE_ALL)]).toEqual([0x1f, 0]);
    expect([...frame(MSG.LOGS, [1])]).toEqual([0x06, 1, 1]);
  });
});

describe("FrameParser", () => {
  it("reassembles frames split across chunks and joins several in one chunk", () => {
    const p = new FrameParser();
    const a = frame(MSG.PONG, [1, 0, 0, 0, 1]), b = frame(MSG.WIFI_ACK);
    expect(p.push(a.slice(0, 3))).toEqual([]);
    const out = p.push(Uint8Array.from([...a.slice(3), ...b]));
    expect(out.map((f) => [f.type, [...f.payload]])).toEqual([[MSG.PONG, [1, 0, 0, 0, 1]], [MSG.WIFI_ACK, []]]);
  });
});

describe("parseHelloAck", () => {
  it("reads version, features and name", () => {
    const info = parseHelloAck(Uint8Array.from([1, 0xe3, ...new TextEncoder().encode("nxc")]));
    expect(info).toEqual({ version: 1, features: 0xe3, name: "nxc" });
  });
});

describe("KEYMAP", () => {
  it("follows the HID usage table on a US layout", () => {
    expect(KEYMAP.get("a")).toEqual([0, 0x04]);
    expect(KEYMAP.get("A")).toEqual([LSHIFT, 0x04]);
    expect(KEYMAP.get("1")).toEqual([0, 0x1e]);
    expect(KEYMAP.get("0")).toEqual([0, 0x27]);
    expect(KEYMAP.get("!")).toEqual([LSHIFT, 0x1e]);
    expect(KEYMAP.get("?")).toEqual([LSHIFT, 0x38]);
    expect(KEYMAP.get("\n")).toEqual([0, 0x28]);
    expect(KEYMAP.get(" ")).toEqual([0, 0x2c]);
    expect(KEYMAP.get("é")).toBeUndefined();
    expect(KEYMAP.get(SPECIAL.SMILEBASIC_DIRECT)).toEqual([LCTRL, 0x27]); // Ctrl+0
  });
});

describe("prepare", () => {
  it("normalises line endings and typographic characters", () => {
    expect(prepare("a\r\nb\rc")).toEqual({ text: "a\nb\nc", skipped: 0 });
    expect(prepare("“hi” — it’s… ok")).toEqual({ text: '"hi" -- it\'s... ok', skipped: 0 });
  });
  it("counts what a US keyboard can't type", () => {
    expect(prepare("héllo ☃")).toEqual({ text: "hllo ", skipped: 2 });
    expect(prepare("a\bb\uE001\uE002")).toEqual({ text: "ab", skipped: 3 }); // special keys never come from the text
  });
});
