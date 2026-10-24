import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { LogBuffer } from "../logs";

const stamp = () => new Date().toISOString().replace(/[-:]/g, "").replace(/\..*/, "").replace("T", "-");

interface Props {
  /** The firmware can send its log (HELLO feature bit): show the section. */
  active: boolean;
  buffer: LogBuffer;
}

export function Logs({ active, buffer }: Props) {
  const [text, setText] = useState(buffer.text);
  const box = useRef<HTMLPreElement>(null);
  const pinned = useRef(true); // stay at the bottom while new output arrives, unless the reader scrolled up

  // Redraw at most once per frame, however fast LOG frames arrive.
  useEffect(() => {
    let raf = 0;
    const off = buffer.subscribe(() => {
      if (!raf) raf = requestAnimationFrame(() => { raf = 0; setText(buffer.text); });
    });
    setText(buffer.text);
    return () => { off(); cancelAnimationFrame(raf); };
  }, [buffer]);

  useLayoutEffect(() => {
    const el = box.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [text, active]);

  const onScroll = () => {
    const el = box.current!;
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
  };

  const download = () => {
    const url = URL.createObjectURL(new Blob([buffer.text], { type: "text/plain;charset=utf-8" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `nxcontroller-log-${stamp()}.txt`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  return (
    <section hidden={!active}>
      <h2>Logs</h2>
      <p class="muted intro">What the board still has in memory (its last 8&nbsp;KB or so), then new output live,
        while this tab is open.</p>
      <div class="row">
        <button class="secondary" onClick={download} disabled={!text}>Download</button>
        <button class="secondary" onClick={() => buffer.clear()} disabled={!text}>Clear</button>
      </div>
      <pre class="log" ref={box} onScroll={onScroll} tabIndex={0}>
        {text || <span class="muted">Waiting for log output…</span>}
      </pre>
    </section>
  );
}
