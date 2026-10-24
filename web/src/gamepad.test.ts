import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BUTTONS, PadController, applyShortcut, axis, hatValue, mergeSources, newSource, newSources, parseSequence,
  readPhysical, type Report,
} from "./gamepad";

describe("hatValue", () => {
  const hat = (...d: string[]) => hatValue(new Set(d));
  it("maps directions to the Switch hat", () => {
    expect(hat("up")).toBe(0);
    expect(hat("up", "right")).toBe(1);
    expect(hat("right")).toBe(2);
    expect(hat("down", "right")).toBe(3);
    expect(hat("down")).toBe(4);
    expect(hat("down", "left")).toBe(5);
    expect(hat("left")).toBe(6);
    expect(hat("up", "left")).toBe(7);
    expect(hat()).toBe(8);
  });
  it("lets opposite directions cancel", () => {
    expect(hat("up", "down")).toBe(8);
    expect(hat("left", "right", "up")).toBe(0);
  });
});

describe("axis", () => {
  it("maps -1..1 to 0..255 around 128", () => {
    expect(axis(-1)).toBe(0);
    expect(axis(0)).toBe(128);
    expect(axis(1)).toBe(255);
    expect(axis(5)).toBe(255);
    expect(axis(-5)).toBe(0);
  });
});

describe("mergeSources", () => {
  it("is neutral when nothing is pressed", () => {
    expect(mergeSources(newSources())).toEqual([0, 0, 8, 128, 128, 128, 128]);
  });
  it("ORs buttons, unions directions and flips stick y", () => {
    const s = newSources();
    s.screen.buttons = BUTTONS.A | BUTTONS.HOME;
    s.keys.buttons = BUTTONS.B;
    s.keys.dirs.add("left");
    s.screen.ls = [0, 1];
    expect(mergeSources(s)).toEqual([0x06, 0x10, 6, 128, 0, 128, 128]);
  });
  it("lets earlier sources win the sticks", () => {
    const s = newSources();
    s.seq.rs = [1, 0];
    s.screen.rs = [-1, 0];
    s.keys.ls = [-1, 0];
    const r = mergeSources(s);
    expect(r[5]).toBe(255);
    expect(r[3]).toBe(0);
  });
});

describe("applyShortcut", () => {
  it("presses and releases buttons, directions and the stick", () => {
    const src = newSource(), held = new Set<string>();
    expect(applyShortcut(src, held, "k", true)).toBe(true);
    expect(src.buttons).toBe(BUTTONS.A);
    applyShortcut(src, held, "k", false);
    expect(src.buttons).toBe(0);
    applyShortcut(src, held, "ArrowUp", true);
    expect([...src.dirs]).toEqual(["up"]);
    applyShortcut(src, held, "w", true);
    applyShortcut(src, held, "d", true);
    expect(src.ls[0]).toBeCloseTo(Math.SQRT1_2);
    expect(src.ls[1]).toBeCloseTo(Math.SQRT1_2);
    applyShortcut(src, held, "w", false);
    applyShortcut(src, held, "d", false);
    expect(src.ls).toEqual([0, 0]);
    expect(applyShortcut(src, held, "z", true)).toBe(false);
  });
});

describe("readPhysical", () => {
  const pad = (pressed: number[], axes: number[]) => ({
    buttons: Array.from({ length: 18 }, (_, i) => ({ pressed: pressed.includes(i) })),
    axes,
  });
  it("maps buttons by position: bottom = B, right = A", () => {
    expect(readPhysical(pad([0], [])).buttons).toBe(BUTTONS.B);
    expect(readPhysical(pad([1], [])).buttons).toBe(BUTTONS.A);
    expect(readPhysical(pad([2, 3], [])).buttons).toBe(BUTTONS.Y | BUTTONS.X);
    expect(readPhysical(pad([16, 17], [])).buttons).toBe(BUTTONS.HOME | BUTTONS.CAPTURE);
    expect([...readPhysical(pad([12, 15], [])).dirs].sort()).toEqual(["right", "up"]);
  });
  it("applies the dead zone and flips y", () => {
    const s = readPhysical(pad([], [0.05, 0.5, 1, -1]));
    expect(s.ls).toEqual([0, -0.5]);
    expect(s.rs).toEqual([1, 1]);
  });
});

describe("parseSequence", () => {
  it("parses buttons, waits and sticks", () => {
    const steps = parseSequence("# comment\nA\nwait 500\nDOWN 200\nL+R 600\nlstick 0 1 400\nrstick -1 0", 100);
    expect(steps).toHaveLength(6);
    expect(steps[0]).toMatchObject({ kind: "press", buttons: BUTTONS.A, hold: 100 });
    expect(steps[1]).toEqual({ kind: "wait", ms: 500 });
    expect(steps[2]).toMatchObject({ kind: "press", buttons: 0, hold: 200 });
    expect([...(steps[2] as { dirs: Set<string> }).dirs]).toEqual(["down"]);
    expect(steps[3]).toMatchObject({ buttons: BUTTONS.L | BUTTONS.R, hold: 600 });
    expect(steps[4]).toEqual({ kind: "stick", stick: "ls", xy: [0, 1], hold: 400 });
    expect(steps[5]).toEqual({ kind: "stick", stick: "rs", xy: [-1, 0], hold: 100 });
  });
  it("reports the line of a mistake", () => {
    expect(() => parseSequence("A\nFOO", 100)).toThrow('line 2: unknown button "FOO"');
    expect(() => parseSequence("wait", 100)).toThrow("line 1: wait needs a duration");
    expect(() => parseSequence("A -5", 100)).toThrow('line 1: bad duration "-5"');
    expect(() => parseSequence("lstick 2 0", 100)).toThrow("between -1 and 1");
  });
});

describe("PadController", () => {
  let now = 0;
  const sent: Report[] = [];
  let connected = true;
  const make = () => new PadController({ canSend: () => connected, send: (r) => sent.push(r), now: () => now });

  beforeEach(() => { vi.useFakeTimers(); now = 0; sent.length = 0; connected = true; });
  afterEach(() => { vi.useRealTimers(); });

  it("sends button changes at once and does not repeat unchanged state", () => {
    const pad = make();
    pad.edit("screen", (s) => { s.buttons = BUTTONS.A; });
    pad.edit("screen", (s) => { s.buttons = BUTTONS.A | BUTTONS.B; });
    pad.update();
    expect(sent.map((r) => r[0])).toEqual([0x04, 0x06]);
  });

  it("throttles stick-only movement to about 60 Hz", () => {
    const pad = make();
    pad.edit("screen", (s) => { s.ls = [0.5, 0]; });   // first state is always sent
    now = 5;
    pad.edit("screen", (s) => { s.ls = [0.6, 0]; });   // too soon: deferred
    now = 8;
    pad.edit("screen", (s) => { s.ls = [0.7, 0]; });
    expect(sent).toHaveLength(1);
    now = 16;
    vi.advanceTimersByTime(11);                         // the deferred send carries the latest value
    expect(sent).toHaveLength(2);
    expect(sent[1]![3]).toBe(axis(0.7));
    now = 17;
    pad.edit("screen", (s) => { s.buttons = BUTTONS.X; }); // a button change skips the throttle
    expect(sent).toHaveLength(3);
  });

  it("sends nothing while disconnected and resends everything after resetSent", () => {
    const pad = make();
    connected = false;
    pad.edit("screen", (s) => { s.buttons = BUTTONS.A; });
    expect(sent).toHaveLength(0);
    connected = true;
    pad.update();
    expect(sent).toHaveLength(1);
    pad.resetSent();
    pad.update();
    expect(sent).toHaveLength(2);
  });

  it("notifies listeners only when what is drawn changes", () => {
    const pad = make();
    const seen: number[] = [];
    pad.subscribe((v) => seen.push(v.report[0]!));
    pad.edit("screen", (s) => { s.buttons = BUTTONS.A; });
    pad.update();
    pad.clear("screen");
    expect(seen).toEqual([0x04, 0]);
  });
});
