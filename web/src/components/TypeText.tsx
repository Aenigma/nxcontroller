import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Link } from "../link";
import { EDITOR_MODES, type EditorMode, adaptForEditor } from "../editor";
import { prepare } from "../protocol";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

interface Props {
  link: Link | null;
  /** A keyboard-capable board is connected: show the section. */
  active: boolean;
  online: boolean;
  typing: boolean;
  setTyping(typing: boolean): void;
}

export function TypeText({ link, active, online, typing, setTyping }: Props) {
  const [text, setText] = useState("");
  const [hold, setHold] = useState("20");
  const [rollover, setRollover] = useState(true);
  const [countdown, setCountdown] = useState("0");
  const [editor, setEditor] = useState<EditorMode>("normal");
  const [tabWidth, setTabWidth] = useState("1");
  const [status, setStatus] = useState("");
  const [progress, setProgress] = useState({ value: 0, max: 1 });
  const [dropping, setDropping] = useState(false);
  const stopRequested = useRef(false);

  // Losing the connection stops typing.
  useEffect(() => { if (!online) stopRequested.current = true; }, [online]);

  /** Keystrokes to send, after US-keyboard normalisation and the editor's quirks. */
  const plan = useMemo(() => {
    const { text: typable, skipped } = prepare(text);
    const opts = { tabWidth: Math.max(1, Math.min(16, Number(tabWidth) || 1)) };
    const keys = adaptForEditor(typable, editor, opts);
    return { keys, skipped, lines: typable ? typable.split("\n").length : 0 };
  }, [text, editor, tabWidth]);

  const stats = !text ? "" :
    `${plan.keys.length.toLocaleString()} keystrokes, ${plan.lines.toLocaleString()} lines` +
    (plan.skipped ? ` · ${plan.skipped} characters can't be typed on a US keyboard and will be skipped` : "");

  const loadFile = async (file: File | undefined) => {
    if (file) setText(await file.text());
  };

  const stop = async () => {
    stopRequested.current = true;
    try { await link?.releaseAll(); } catch { /* clears the board's queue at once */ }
  };

  const type = async () => {
    if (!link) return;
    const { keys } = plan;
    if (!keys) return;
    const holdMs = Math.max(0, Math.min(255, Number(hold) || 0));
    const secsBefore = Math.max(0, Number(countdown) || 0);
    stopRequested.current = false;
    setTyping(true);
    setProgress({ value: 0, max: keys.length });
    try {
      for (let i = secsBefore; i > 0 && !stopRequested.current; i--) { setStatus(`Starting in ${i}…`); await sleep(1000); }
      const start = performance.now();
      const finished = await link.type(keys, {
        holdMs, rollover,
        shouldStop: () => stopRequested.current,
        onProgress: (done, total) => {
          setProgress({ value: done, max: total });
          const secs = (performance.now() - start) / 1000, rate = done / Math.max(secs, 0.001);
          setStatus(`${done.toLocaleString()} / ${total.toLocaleString()} · ${rate.toFixed(0)} chars/s · ` +
                    `${Math.ceil((total - done) / Math.max(rate, 1))} s left`);
        },
      });
      const secs = (performance.now() - start) / 1000;
      setStatus(finished ? `Done: ${keys.length.toLocaleString()} keystrokes in ${secs.toFixed(1)} s.` : "Stopped.");
    } catch (e) {
      setStatus("Typing failed: " + (e as Error).message);
      try { await link.releaseAll(); } catch { /* already gone */ }
    }
    setTyping(false);
  };

  return (
    <section hidden={!active}>
      <h2>Type text</h2>
      <p class="muted intro">Pick a file, drop one on the box, or paste. The board types it as a USB keyboard
        into whatever has focus: on a Switch, a text field.</p>
      <div class="row">
        <input type="file" accept=".txt,.md,.csv,.json,.py,.rs,.js,.ts,.html,.css,text/*"
          onChange={(e) => loadFile(e.currentTarget.files?.[0])} />
        <span class="muted">{stats}</span>
      </div>
      <textarea
        class={dropping ? "drop" : ""}
        spellcheck={false}
        placeholder="Text to type"
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onDragOver={(e) => { e.preventDefault(); setDropping(true); }}
        onDragLeave={() => setDropping(false)}
        onDrop={(e) => { e.preventDefault(); setDropping(false); loadFile(e.dataTransfer?.files[0]); }}
      />
      <div class="row mt12">
        <label>Editor <select value={editor} onChange={(e) => setEditor(e.currentTarget.value as EditorMode)}>
          {EDITOR_MODES.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
        </select></label>
        {editor === "smilebasic" &&
          <label>Tab <input type="number" min="1" max="16" value={tabWidth} onInput={(e) => setTabWidth(e.currentTarget.value)} /> spaces</label>}
        <span class="muted">{EDITOR_MODES.find((m) => m.id === editor)!.hint}</span>
      </div>
      <div class="row mt12">
        <label>Hold <input type="number" min="0" max="255" value={hold} onInput={(e) => setHold(e.currentTarget.value)} /> ms</label>
        <label><input type="checkbox" checked={rollover} onChange={(e) => setRollover(e.currentTarget.checked)} /> Rollover</label>
        <label>Countdown <input type="number" min="0" max="30" value={countdown} onInput={(e) => setCountdown(e.currentTarget.value)} /> s</label>
      </div>
      <div class="row mt14">
        <button onClick={type} disabled={!active || typing || !text}>Type</button>
        <button class="secondary" onClick={stop} disabled={!typing}>Stop</button>
        <span class="muted">{status}</span>
      </div>
      <progress value={progress.value} max={progress.max} />
      <p class="step-note"><b>Hold</b> is how long each key stays down and up: 20&nbsp;ms is reliable on a Switch 2,
        0 is fastest on a computer. <b>Rollover</b> goes straight from one key to the next, about twice as fast.{" "}
        <b>Countdown</b> gives you time to click into the target window. <b>Editor</b> makes up for what the target
        editor does on its own (see Help, under Setup). Characters a US keyboard can't type are skipped.</p>
    </section>
  );
}
