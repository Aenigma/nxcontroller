import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import {
  BUTTONS, PadController, applyShortcut, newSource, normalizeKey, parseSequence, readPhysical, runSequence,
  type ButtonName, type Dir, type PadView, type Report,
} from "../gamepad";
import type { Link } from "../link";
import { FEATURE, MSG, frame } from "../protocol";

const SEQ_DEFAULT = `# One step per line. Buttons: A B X Y L R ZL ZR PLUS MINUS HOME CAPTURE LSTICK RSTICK
# D-pad: UP DOWN LEFT RIGHT. Combine with +. A number is how long to hold (ms).
A
wait 500
DOWN 200
L+R 600
lstick 0 1 400
`;

// Keep getting a pointer's events while it's held, even off the element.
function capture(el: Element, e: PointerEvent) {
  try { el.setPointerCapture(e.pointerId); } catch { /* not every pointer can be captured */ }
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

interface HoldProps {
  cls?: string;
  on: boolean;
  title?: string;
  label?: string;
  onHold(down: boolean): void;
  children: ComponentChildren;
}

/** A button that is pressed while held (pointer capture handles multi-touch). */
function Hold({ cls = "", on, title, label, onHold, children }: HoldProps) {
  const end = () => onHold(false);
  return (
    <button
      class={["pb", cls, on && "on"].filter(Boolean).join(" ")}
      title={title}
      aria-label={label}
      onPointerDown={(e) => { e.preventDefault(); capture(e.currentTarget, e); onHold(true); }}
      onPointerUp={end}
      onPointerCancel={end}
      onLostPointerCapture={end}
      onContextMenu={(e) => e.preventDefault()}
    >{children}</button>
  );
}

interface StickProps {
  /** Knob position, -1..1 on both axes, +y down (as drawn). */
  x: number;
  y: number;
  onMove(xy: [number, number]): void;
  onRelease(): void;
}

/** Drag the knob; it springs back on release. */
function Stick({ x, y, onMove, onRelease }: StickProps) {
  const el = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const move = (e: PointerEvent) => {
    const box = el.current!.getBoundingClientRect(), r = box.width / 2;
    let nx = (e.clientX - box.left - r) / (r - 24), ny = -(e.clientY - box.top - r) / (r - 24);
    const len = Math.hypot(nx, ny);
    if (len > 1) { nx /= len; ny /= len; }
    onMove([nx, ny]);
  };
  const end = () => { dragging.current = false; onRelease(); };
  return (
    <div
      class={`stick${x !== 0 || y !== 0 ? " on" : ""}`}
      ref={el}
      onPointerDown={(e) => { e.preventDefault(); capture(e.currentTarget, e); dragging.current = true; move(e); }}
      onPointerMove={(e) => { if (dragging.current) move(e); }}
      onPointerUp={end}
      onPointerCancel={end}
      onLostPointerCapture={end}
    >
      <div class="knob" style={{ transform: `translate(calc(var(--r) * ${x}), calc(var(--r) * ${y}))` }} />
    </div>
  );
}

interface Props {
  link: Link | null;
  /** A gamepad-capable board is connected: show the section. */
  active: boolean;
}

export function Gamepad({ link, active }: Props) {
  const linkRef = useRef(link);
  linkRef.current = link;
  const activeRef = useRef(active);
  activeRef.current = active;

  const padRef = useRef<PadController | null>(null);
  if (!padRef.current) {
    let seenOpen = 0;
    const pad: PadController = new PadController({
      canSend() {
        const l = linkRef.current;
        if (!l?.connected || !(l.features & FEATURE.GAMEPAD)) return false;
        if (l.opened !== seenOpen) { seenOpen = l.opened; pad.resetSent(); } // resend the full state after (re)connecting
        return true;
      },
      send(report: Report) { linkRef.current?.send(frame(MSG.GAMEPAD, report)).catch(() => {}); },
    });
    padRef.current = pad;
  }
  const pad = padRef.current;

  const [view, setView] = useState<PadView>(pad.view);
  useEffect(() => { setView(pad.view); return pad.subscribe(setView); }, [pad]);
  useEffect(() => () => pad.dispose(), [pad]);

  // Keyboard shortcuts (not while typing in a text box).
  useEffect(() => {
    const held = new Set<string>();
    const onKey = (e: KeyboardEvent, down: boolean) => {
      const inField = e.target instanceof Element && e.target.closest("textarea, input, select");
      if (!activeRef.current || inField || e.metaKey || e.ctrlKey || e.altKey) return;
      if (!applyShortcut(pad.sources.keys, held, normalizeKey(e.key), down)) return;
      e.preventDefault();
      if (!e.repeat) pad.update();
    };
    const keydown = (e: KeyboardEvent) => onKey(e, true);
    const keyup = (e: KeyboardEvent) => onKey(e, false);
    // Never leave inputs held when the page loses focus.
    const release = () => { held.clear(); pad.clear("keys"); pad.clear("screen"); };
    const hidden = () => { if (document.hidden) release(); };
    window.addEventListener("keydown", keydown);
    window.addEventListener("keyup", keyup);
    window.addEventListener("blur", release);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      window.removeEventListener("keydown", keydown);
      window.removeEventListener("keyup", keyup);
      window.removeEventListener("blur", release);
      document.removeEventListener("visibilitychange", hidden);
    };
  }, [pad]);

  // A controller plugged into this computer.
  const [usePad, setUsePad] = useState(false);
  const [padStatus, setPadStatus] = useState("");
  useEffect(() => {
    if (!usePad) return;
    let raf = 0;
    const poll = () => {
      const gp = [...navigator.getGamepads()].find((g) => g && g.connected);
      if (gp) {
        setPadStatus(`Using ${gp.id.replace(/\s*\(.*\)\s*$/, "")}`);
        pad.set("phys", readPhysical(gp));
      } else {
        setPadStatus("No controller found: plug one in and press a button on it.");
        pad.clear("phys");
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);
    return () => { cancelAnimationFrame(raf); setPadStatus(""); pad.clear("phys"); };
  }, [usePad, pad]);

  // Sequences.
  const [seq, setSeq] = useState(SEQ_DEFAULT);
  const [seqHold, setSeqHold] = useState("100");
  const [seqGap, setSeqGap] = useState("100");
  const [seqRepeat, setSeqRepeat] = useState("1");
  const [seqStatus, setSeqStatus] = useState("");
  const [running, setRunning] = useState(false);
  const stopSeq = useRef(false);

  const run = async () => {
    const tap = Number(seqHold) || 100, gap = Number(seqGap) || 0;
    const repeat = Math.max(1, Number(seqRepeat) || 1);
    let steps;
    try { steps = parseSequence(seq, tap); }
    catch (e) { setSeqStatus((e as Error).message); return; }
    stopSeq.current = false;
    setRunning(true);
    try {
      const finished = await runSequence(pad, steps, {
        gap, repeat, shouldStop: () => stopSeq.current, onStatus: setSeqStatus,
      });
      setSeqStatus(finished ? "Done." : "Stopped.");
    } finally {
      setRunning(false);
    }
  };

  const register = async () => {
    // A Switch pairs a new wired pad when L and R are pressed together on
    // Controllers → Change Grip/Order.
    pad.set("seq", { ...newSource(), buttons: BUTTONS.L | BUTTONS.R });
    await sleep(600);
    pad.clear("seq");
  };

  // What's drawn.
  const [report, dirs] = [view.report, new Set<Dir>(view.dirs)];
  const buttons = (report[0] ?? 0) | ((report[1] ?? 0) << 8);
  const knob = (ix: number, iy: number) => ({ x: ((report[ix] ?? 128) - 128) / 127.5, y: ((report[iy] ?? 128) - 128) / 127.5 });

  const B = (name: ButtonName, props: { cls?: string; title?: string; children: ComponentChildren }) => (
    <Hold cls={props.cls} title={props.title} on={!!(buttons & BUTTONS[name])}
      onHold={(down) => pad.edit("screen", (s) => { s.buttons = down ? s.buttons | BUTTONS[name] : s.buttons & ~BUTTONS[name]; })}>
      {props.children}
    </Hold>
  );
  const D = (dir: string, children: ComponentChildren, label?: string) => {
    const want = dir.split(",") as Dir[];
    return (
      <Hold cls="dpad" label={label} on={want.every((d) => dirs.has(d)) && dirs.size === want.length}
        onHold={(down) => pad.edit("screen", (s) => want.forEach((d) => (down ? s.dirs.add(d) : s.dirs.delete(d))))}>
        {children}
      </Hold>
    );
  };
  const S = (side: "ls" | "rs") => {
    const k = side === "ls" ? knob(3, 4) : knob(5, 6);
    return (
      <Stick x={k.x} y={k.y}
        onMove={(xy) => pad.edit("screen", (s) => { s[side] = xy; })}
        onRelease={() => pad.edit("screen", (s) => { s[side] = [0, 0]; })} />
    );
  };
  const num = (set: (v: string) => void) => (e: { currentTarget: HTMLInputElement }) => set(e.currentTarget.value);

  return (
    <section hidden={!active}>
      <h2>Gamepad</h2>
      <p class="muted intro">On a Switch, open <b>Controllers → Change Grip/Order</b> and click
        <b>Register</b> once. Hold buttons on screen, use the keyboard, or pass through a controller plugged into this computer.</p>
      <div class="row">
        <button class="secondary" onClick={register} disabled={!active || running}>Register (L + R)</button>
        <label><input type="checkbox" checked={usePad} onChange={(e) => setUsePad(e.currentTarget.checked)} /> Use a controller plugged into this computer</label>
        <span class="muted">{padStatus}</span>
      </div>

      <div class="pad">
        <div class="pad-row">
          <div class="mini">{B("ZL", { cls: "shoulder", children: "ZL" })}{B("L", { cls: "shoulder", children: "L" })}</div>
          <div class="mini">{B("R", { cls: "shoulder", children: "R" })}{B("ZR", { cls: "shoulder", children: "ZR" })}</div>
        </div>
        <div class="pad-row mt14">
          <div class="pad-half">
            <div class="mini">{B("MINUS", { cls: "small", children: "−" })}{B("CAPTURE", { cls: "small", children: "◉" })}</div>
            {S("ls")}
            <div class="cluster">
              {D("up,left", "↖", "up-left")}
              {D("up", "↑")}
              {D("up,right", "↗", "up-right")}
              {D("left", "←")}
              {B("LSTICK", { cls: "small", title: "Left stick click", children: "LS" })}
              {D("right", "→")}
              {D("down,left", "↙", "down-left")}
              {D("down", "↓")}
              {D("down,right", "↘", "down-right")}
            </div>
          </div>
          <div class="pad-half">
            <div class="mini">{B("HOME", { cls: "small", children: "⌂" })}{B("PLUS", { cls: "small", children: "+" })}</div>
            <div class="cluster">
              <span />{B("X", { children: "X" })}<span />
              {B("Y", { children: "Y" })}{B("RSTICK", { cls: "small", title: "Right stick click", children: "RS" })}{B("A", { children: "A" })}
              <span />{B("B", { children: "B" })}<span />
            </div>
            {S("rs")}
          </div>
        </div>
      </div>
      <p class="muted pad-help">
        Keyboard (click the page first): <kbd>←↑→↓</kbd> D-pad · <kbd>W</kbd><kbd>A</kbd><kbd>S</kbd><kbd>D</kbd> left stick ·
        <kbd>K</kbd> A · <kbd>J</kbd> B · <kbd>I</kbd> X · <kbd>U</kbd> Y · <kbd>Q</kbd>/<kbd>E</kbd> L/R ·
        <kbd>1</kbd>/<kbd>3</kbd> ZL/ZR · <kbd>Enter</kbd> + · <kbd>⌫</kbd> − · <kbd>H</kbd> Home
      </p>

      <h3>Sequence</h3>
      <textarea id="seq" spellcheck={false} value={seq} onInput={(e) => setSeq(e.currentTarget.value)} />
      <div class="row mt12">
        <label>Tap <input type="number" min="10" max="5000" value={seqHold} onInput={num(setSeqHold)} /> ms</label>
        <label>Gap <input type="number" min="0" max="5000" value={seqGap} onInput={num(setSeqGap)} /> ms</label>
        <label>Repeat <input type="number" min="1" max="9999" value={seqRepeat} onInput={num(setSeqRepeat)} /></label>
        <button onClick={run} disabled={!active || running}>Run</button>
        <button class="secondary" onClick={() => { stopSeq.current = true; }} disabled={!running}>Stop</button>
        <span class="muted">{seqStatus}</span>
      </div>
    </section>
  );
}
