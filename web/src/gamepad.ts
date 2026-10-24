// Gamepad state: merging input sources into one report, hat/axis math, keyboard
// shortcuts, a physical pad, and the sequence parser/runner. Framework-free.

// Switch / HORIPAD layout (PROTOCOL.md).
export const BUTTONS = {
  Y: 0x0001, B: 0x0002, A: 0x0004, X: 0x0008, L: 0x0010, R: 0x0020, ZL: 0x0040, ZR: 0x0080,
  MINUS: 0x0100, PLUS: 0x0200, LSTICK: 0x0400, RSTICK: 0x0800, HOME: 0x1000, CAPTURE: 0x2000,
} as const;
export type ButtonName = keyof typeof BUTTONS;

export type Dir = "up" | "down" | "left" | "right";
export const DIRS: readonly Dir[] = ["up", "down", "left", "right"];

/** x, y in -1..1; +y is up on screen (the wire has 0 as up, see `axis`). */
export type StickPos = readonly [x: number, y: number];

export interface Source {
  buttons: number;
  dirs: Set<Dir>;
  ls: StickPos;
  rs: StickPos;
}
export const newSource = (): Source => ({ buttons: 0, dirs: new Set(), ls: [0, 0], rs: [0, 0] });

/** Every input method keeps its own state; the board gets them merged. Earlier sources win the sticks. */
export const SOURCE_ORDER = ["seq", "phys", "screen", "keys"] as const;
export type SourceName = (typeof SOURCE_ORDER)[number];
export type Sources = Record<SourceName, Source>;
export const newSources = (): Sources => ({ seq: newSource(), phys: newSource(), screen: newSource(), keys: newSource() });

/** GAMEPAD payload: buttons lo, buttons hi, hat, lx, ly, rx, ry. */
export type Report = number[];

export function hatValue(dirs: ReadonlySet<string>): number {
  const up = dirs.has("up") && !dirs.has("down"), down = dirs.has("down") && !dirs.has("up");
  const left = dirs.has("left") && !dirs.has("right"), right = dirs.has("right") && !dirs.has("left");
  if (up) return right ? 1 : left ? 7 : 0;
  if (down) return right ? 3 : left ? 5 : 4;
  if (right) return 2;
  if (left) return 6;
  return 8;
}

/** -1 -> 0, 0 -> 128, +1 -> 255 */
export function axis(v: number): number {
  v = Math.max(-1, Math.min(1, v));
  return Math.round(128 + v * (v < 0 ? 128 : 127));
}

export function mergeSources(sources: Sources): Report {
  let buttons = 0;
  const dirs = new Set<Dir>();
  let ls: StickPos = [0, 0], rs: StickPos = [0, 0];
  for (const name of SOURCE_ORDER) {
    const src = sources[name];
    buttons |= src.buttons;
    src.dirs.forEach((d) => dirs.add(d));
    if (ls[0] === 0 && ls[1] === 0) ls = src.ls;
    if (rs[0] === 0 && rs[1] === 0) rs = src.rs;
  }
  // Stick y: +1 is up on screen, 0 is up on the wire.
  return [buttons & 0xff, buttons >> 8, hatValue(dirs), axis(ls[0]), axis(-ls[1]), axis(rs[0]), axis(-rs[1])];
}

/** What to draw: the merged report plus every pressed direction (D-pad buttons light per direction). */
export interface PadView { report: Report; dirs: Dir[] }

export function viewOf(sources: Sources): PadView {
  const dirs = new Set<Dir>();
  Object.values(sources).forEach((s) => s.dirs.forEach((d) => dirs.add(d)));
  return { report: mergeSources(sources), dirs: DIRS.filter((d) => dirs.has(d)) };
}

const sameReport = (a: readonly number[], b: readonly number[]) => a.every((v, i) => v === b[i]);
export const STICK_INTERVAL_MS = 16;

export interface PadIO {
  /** True while a gamepad-capable board is connected. */
  canSend(): boolean;
  send(report: Report): void;
  now?(): number;
}

/** Owns the input sources and sends the merged state to the board. */
export class PadController {
  readonly sources = newSources();
  private lastSent: Report | null = null;
  private lastSentAt = 0;
  private sendTimer: ReturnType<typeof setTimeout> | null = null;
  private viewKey = "";
  private listeners = new Set<(view: PadView) => void>();

  constructor(private readonly io: PadIO) {}

  private now(): number { return this.io.now ? this.io.now() : performance.now(); }

  get view(): PadView { return viewOf(this.sources); }

  subscribe(fn: (view: PadView) => void): () => void {
    this.listeners.add(fn);
    return () => { this.listeners.delete(fn); };
  }

  /** Change a source in place, then send and redraw. */
  edit(name: SourceName, fn: (src: Source) => void): void {
    fn(this.sources[name]);
    this.update();
  }

  set(name: SourceName, src: Source): void {
    this.sources[name] = src;
    this.update();
  }

  clear(name: SourceName): void { this.set(name, newSource()); }

  /** Resend the full state next time (after connecting, say). */
  resetSent(): void { this.lastSent = null; }

  dispose(): void {
    if (this.sendTimer) clearTimeout(this.sendTimer);
    this.sendTimer = null;
    this.listeners.clear();
  }

  update(): void {
    this.sendIfChanged();
    const view = this.view;
    const key = view.report.join(",") + "|" + view.dirs.join(",");
    if (key !== this.viewKey) {
      this.viewKey = key;
      this.listeners.forEach((fn) => fn(view));
    }
  }

  /** Send the merged state if it changed. Button/D-pad changes go out at once;
   *  stick-only movement is limited to about 60 updates a second. */
  private sendIfChanged(): void {
    if (!this.io.canSend()) return;
    const report = mergeSources(this.sources);
    const last = this.lastSent;
    if (last && sameReport(report, last)) return;
    const discrete = !last || report[0] !== last[0] || report[1] !== last[1] || report[2] !== last[2];
    const now = this.now();
    if (!discrete && now - this.lastSentAt < STICK_INTERVAL_MS) {
      if (!this.sendTimer) {
        this.sendTimer = setTimeout(() => { this.sendTimer = null; this.sendIfChanged(); }, STICK_INTERVAL_MS - (now - this.lastSentAt));
      }
      return;
    }
    this.lastSent = report;
    this.lastSentAt = now;
    this.io.send(report);
  }
}

// ---------------------------------------------------------------- keyboard shortcuts
const KEY_BUTTONS = new Map<string, ButtonName>([
  ["k", "A"], ["j", "B"], ["i", "X"], ["u", "Y"], ["q", "L"], ["e", "R"], ["1", "ZL"], ["3", "ZR"],
  ["Enter", "PLUS"], ["Backspace", "MINUS"], ["h", "HOME"],
]);
const KEY_DIRS = new Map<string, Dir>([["ArrowUp", "up"], ["ArrowDown", "down"], ["ArrowLeft", "left"], ["ArrowRight", "right"]]);
const KEY_STICK = new Map<string, StickPos>([["w", [0, 1]], ["s", [0, -1]], ["a", [-1, 0]], ["d", [1, 0]]]);

/** Single characters are matched case-insensitively. */
export const normalizeKey = (key: string): string => (key.length === 1 ? key.toLowerCase() : key);

/** Apply a key press/release to the "keys" source. Returns false if it isn't a shortcut. */
export function applyShortcut(src: Source, heldStick: Set<string>, key: string, down: boolean): boolean {
  const button = KEY_BUTTONS.get(key), dir = KEY_DIRS.get(key);
  if (button) {
    src.buttons = down ? src.buttons | BUTTONS[button] : src.buttons & ~BUTTONS[button];
  } else if (dir) {
    if (down) src.dirs.add(dir); else src.dirs.delete(dir);
  } else if (KEY_STICK.has(key)) {
    if (down) heldStick.add(key); else heldStick.delete(key);
    let x = 0, y = 0;
    heldStick.forEach((h) => { const v = KEY_STICK.get(h)!; x += v[0]; y += v[1]; });
    const len = Math.hypot(x, y) || 1;
    src.ls = [x / len, y / len];
  } else {
    return false;
  }
  return true;
}

// ---------------------------------------------------------------- a controller plugged into this computer
// Gamepad API "standard" mapping. Buttons map by position, Nintendo style: bottom = B, right = A, left = Y, top = X.
export const PHYS_BUTTONS: readonly ButtonName[] =
  ["B", "A", "Y", "X", "L", "R", "ZL", "ZR", "MINUS", "PLUS", "LSTICK", "RSTICK"];
export const DEADZONE = 0.12;
const dz = (v: number) => (Math.abs(v) < DEADZONE ? 0 : v);

export interface PhysicalPad {
  buttons: ReadonlyArray<{ pressed: boolean }>;
  axes: ReadonlyArray<number>;
}

export function readPhysical(gp: PhysicalPad): Source {
  const src = newSource();
  PHYS_BUTTONS.forEach((name, i) => { if (gp.buttons[i]?.pressed) src.buttons |= BUTTONS[name]; });
  if (gp.buttons[16]?.pressed) src.buttons |= BUTTONS.HOME;
  if (gp.buttons[17]?.pressed) src.buttons |= BUTTONS.CAPTURE;
  const pad: Array<[number, Dir]> = [[12, "up"], [13, "down"], [14, "left"], [15, "right"]];
  pad.forEach(([i, d]) => { if (gp.buttons[i]?.pressed) src.dirs.add(d); });
  src.ls = [dz(gp.axes[0] ?? 0), -dz(gp.axes[1] ?? 0)];
  src.rs = [dz(gp.axes[2] ?? 0), -dz(gp.axes[3] ?? 0)];
  return src;
}

// ---------------------------------------------------------------- sequences
export type Step =
  | { kind: "wait"; ms: number }
  | { kind: "stick"; stick: "ls" | "rs"; xy: StickPos; hold: number }
  | { kind: "press"; buttons: number; dirs: Set<Dir>; hold: number };

/** One step per line: buttons joined with +, `wait ms`, `lstick x y [ms]`. `#` starts a comment. */
export function parseSequence(text: string, tapMs: number): Step[] {
  const steps: Step[] = [];
  text.split("\n").forEach((raw, i) => {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) return;
    const parts = line.split(/\s+/), head = parts[0]!.toLowerCase();
    const fail = (msg: string): never => { throw new Error(`line ${i + 1}: ${msg}`); };
    const ms = (v: string | undefined, def?: number): number => {
      if (v === undefined) return def as number;
      const n = Number(v);
      if (!(n >= 0)) fail(`bad duration "${v}"`);
      return n;
    };
    if (head === "wait") {
      if (parts[1] === undefined) fail("wait needs a duration");
      steps.push({ kind: "wait", ms: ms(parts[1]) });
      return;
    }
    if (head === "lstick" || head === "rstick") {
      const x = Number(parts[1]), y = Number(parts[2]);
      if (!(Math.abs(x) <= 1 && Math.abs(y) <= 1)) fail("stick x and y must be between -1 and 1");
      steps.push({ kind: "stick", stick: head === "lstick" ? "ls" : "rs", xy: [x, y], hold: ms(parts[3], tapMs) });
      return;
    }
    const step = { kind: "press" as const, buttons: 0, dirs: new Set<Dir>(), hold: ms(parts[1], tapMs) };
    for (const name of parts[0]!.toUpperCase().split("+")) {
      const dir = name.toLowerCase() as Dir;
      if (Object.hasOwn(BUTTONS, name)) step.buttons |= BUTTONS[name as ButtonName];
      else if (DIRS.includes(dir)) step.dirs.add(dir);
      else fail(`unknown button "${name}"`);
    }
    steps.push(step);
  });
  return steps;
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export async function sleepUnless(ms: number, shouldStop: () => boolean): Promise<void> {
  const end = performance.now() + ms;
  while (!shouldStop() && performance.now() < end) await sleep(Math.min(20, end - performance.now()));
}

export interface RunOptions {
  gap: number;
  repeat: number;
  shouldStop(): boolean;
  onStatus(msg: string): void;
}

/** Play `steps` through the "seq" source. Releases it when done. Returns true if it ran to the end. */
export async function runSequence(pad: PadController, steps: Step[], { gap, repeat, shouldStop, onStatus }: RunOptions): Promise<boolean> {
  try {
    for (let round = 1; round <= repeat && !shouldStop(); round++) {
      for (let i = 0; i < steps.length && !shouldStop(); i++) {
        onStatus(`Round ${round}/${repeat}, step ${i + 1}/${steps.length}`);
        const step = steps[i]!, src = newSource();
        if (step.kind === "wait") { await sleepUnless(step.ms, shouldStop); continue; }
        if (step.kind === "stick") src[step.stick] = step.xy; else { src.buttons = step.buttons; src.dirs = step.dirs; }
        pad.set("seq", src);
        await sleepUnless(step.hold, shouldStop);
        pad.clear("seq");
        await sleepUnless(gap, shouldStop);
      }
    }
    return !shouldStop();
  } finally {
    pad.clear("seq");
  }
}
