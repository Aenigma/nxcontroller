import { useEffect, useRef, useState } from "preact/hooks";
import { BoardSettings, ConnectBar, type Status } from "./components/Connect";
import { Gamepad } from "./components/Gamepad";
import { Help } from "./components/Help";
import { Install } from "./components/Install";
import { Logs } from "./components/Logs";
import { TypeText } from "./components/TypeText";
import { Wifi } from "./components/Wifi";
import { Link } from "./link";
import { LogBuffer } from "./logs";
import { FEATURE, MODE_NAMES } from "./protocol";

type Tab = "control" | "setup";
const TABS: { id: Tab; label: string }[] = [
  { id: "control", label: "Control" },
  { id: "setup", label: "Setup" },
];

// Remembered per browser: a board has connected before, so open on Control.
const CONNECTED_KEY = "nxcontroller.connected-before";
const remembered = (): boolean => { try { return localStorage.getItem(CONNECTED_KEY) === "1"; } catch { return false; } };
const remember = () => { try { localStorage.setItem(CONNECTED_KEY, "1"); } catch { /* storage unavailable */ } };

/** The tab named in the URL (#control / #setup), else Control once a board has connected, else Setup. */
function initialTab(): Tab {
  const hash = location.hash.slice(1);
  if (hash === "control" || hash === "setup") return hash;
  return remembered() ? "control" : "setup";
}

export function App() {
  const unsupported = !navigator.bluetooth;
  const linkRef = useRef<Link | null>(null);
  const logs = useRef(new LogBuffer()).current;

  const [link, setLink] = useState<Link | null>(null);
  const [online, setOnline] = useState(false);
  const [features, setFeatures] = useState(0);
  const [status, setStatusState] = useState<Status>({ msg: "Not connected.", cls: "muted" });
  const [mode, setMode] = useState("3");
  const [applying, setApplying] = useState(false);
  const [typing, setTyping] = useState(false);
  const [tab, setTabState] = useState<Tab>(initialTab);

  const setTab = (t: Tab) => {
    setTabState(t);
    history.replaceState(null, "", `#${t}`);
  };
  useEffect(() => {
    const onHash = () => { const h = location.hash.slice(1); if (h === "control" || h === "setup") setTabState(h); };
    addEventListener("hashchange", onHash);
    return () => removeEventListener("hashchange", onHash);
  }, []);

  // Stream the board's log only while the Setup tab (where the Logs section lives) is open,
  // so nothing extra travels over Bluetooth while you play.
  const wantLogs = tab === "setup" && online && !!(features & FEATURE.LOGS);
  useEffect(() => {
    if (!link) return;
    if (wantLogs) logs.reset(); // the board replays everything it has first
    void link.followLogs(wantLogs);
  }, [link, wantLogs]);

  const setStatus = (msg: string, cls = "muted") => setStatusState({ msg, cls });

  /** Show what the link can do right now: section visibility and enabled buttons. */
  const refresh = () => {
    const l = linkRef.current;
    setOnline(!!l?.connected);
    setFeatures(l?.connected ? l.features : 0);
  };

  /** Read the board's state into the status line and mode picker. */
  const describe = async (l: Link) => {
    const { usbConfigured } = await l.ping();
    const feat = l.features;
    setMode(String(feat & 0x03));
    setStatus(`Connected to ${l.info?.name} · ${MODE_NAMES[feat & 0x03] ?? "unknown"} · USB ${usbConfigured ? "plugged in" : "not plugged into anything"}`,
              usbConfigured ? "status-ok" : "muted");
    if (!(feat & FEATURE.PACED_TYPING)) setStatus("This firmware is too old for typing from here: update it.", "status-bad");
  };

  const connect = async () => {
    try {
      setStatus("Choose your board in the browser's list…");
      const l = new Link({
        onDisconnect: () => { setStatus("Disconnected.", "status-bad"); refresh(); },
        onLog: (payload) => logs.push(payload),
        onOpen: () => logs.reset(), // after a restart, a followed log replays from the start
      });
      linkRef.current = l;
      setLink(l);
      await l.connect();
      remember();
      await describe(l);
    } catch (e) {
      const err = e as Error;
      setStatus(err.name === "NotFoundError" ? "No board chosen." : "Couldn't connect: " + err.message, "status-bad");
      linkRef.current?.disconnect();
    }
    refresh();
  };

  const applyMode = async () => {
    const l = linkRef.current;
    if (!l) return;
    setStatus("Switching mode; the board restarts…");
    setApplying(true);
    try { await l.setMode(Number(mode)); await describe(l); }
    catch (e) { setStatus("Mode change failed: " + (e as Error).message, "status-bad"); }
    setApplying(false);
    refresh();
  };

  const saveWifi = async (ssid: string, password: string) => {
    const l = linkRef.current;
    if (!l) throw new Error("not connected");
    try {
      await l.setWifi(ssid, password);
      await describe(l);
    } finally {
      refresh();
    }
  };

  const gamepad = online && !!(features & FEATURE.GAMEPAD);
  const keyboard = online && !!(features & FEATURE.KEYBOARD);

  return (
    <main>
      <h1>nxcontroller</h1>
      <p class="lede">Turn an ESP32-S3 board into a USB gamepad and keyboard for a Nintendo Switch or a computer,
        and control it from this page over Bluetooth.</p>

      {unsupported && (
        <p class="warn">This browser can't use Bluetooth or USB devices. Use Chrome or Edge on a computer
          (Chrome on Android can connect, but not install).</p>
      )}

      <ConnectBar
        online={online} unsupported={unsupported} status={status}
        onConnect={connect} onDisconnect={() => linkRef.current?.disconnect()}
      />

      <nav class="tabs" role="tablist">
        {TABS.map((t) => (
          <button key={t.id} role="tab" aria-selected={tab === t.id} onClick={() => setTab(t.id)}>{t.label}</button>
        ))}
      </nav>

      <div role="tabpanel" hidden={tab !== "control"}>
        {!online && (
          <p class="muted hint">Connect your board to use it as a gamepad or keyboard.
            New board? Install the firmware in <a href="#setup" onClick={(e) => { e.preventDefault(); setTab("setup"); }}>Setup</a>.</p>
        )}
        <Gamepad link={link} active={gamepad} />
        <TypeText link={link} active={keyboard} online={online} typing={typing} setTyping={setTyping} />
      </div>

      <div role="tabpanel" hidden={tab !== "setup"}>
        <Install />
        <BoardSettings
          online={online} typing={typing} applying={applying}
          mode={mode} onMode={setMode} onApply={applyMode}
        />
        <Wifi active={online && !!(features & FEATURE.WIFI)} online={online} typing={typing} onSave={saveWifi} />
        <Logs active={online && !!(features & FEATURE.LOGS)} buffer={logs} />
        <Help />
      </div>
    </main>
  );
}
