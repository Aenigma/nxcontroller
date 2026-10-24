import { useRef, useState } from "preact/hooks";
import { Connect, type Status } from "./components/Connect";
import { Gamepad } from "./components/Gamepad";
import { Help } from "./components/Help";
import { Install } from "./components/Install";
import { Logs } from "./components/Logs";
import { TypeText } from "./components/TypeText";
import { Wifi } from "./components/Wifi";
import { Link } from "./link";
import { LogBuffer } from "./logs";
import { FEATURE, MODE_NAMES } from "./protocol";

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
        onOpen: () => logs.reset(), // the board replays its log on every connection
      });
      linkRef.current = l;
      setLink(l);
      await l.connect();
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

  return (
    <main>
      <h1>nxcontroller</h1>
      <p class="lede">Turn an ESP32-S3 board into a USB gamepad and keyboard for a Nintendo Switch or a computer,
        and control it from this page over Bluetooth.</p>

      {unsupported && (
        <p class="warn">This browser can't use Bluetooth or USB devices. Use Chrome or Edge on a computer
          (Chrome on Android can connect, but not install).</p>
      )}

      <Install />
      <Connect
        online={online} typing={typing} applying={applying} unsupported={unsupported}
        status={status} mode={mode} onMode={setMode}
        onConnect={connect} onDisconnect={() => linkRef.current?.disconnect()} onApply={applyMode}
      />
      <Gamepad link={link} active={online && !!(features & FEATURE.GAMEPAD)} />
      <TypeText link={link} active={online && !!(features & FEATURE.KEYBOARD)} online={online} typing={typing} setTyping={setTyping} />
      <Wifi active={online && !!(features & FEATURE.WIFI)} online={online} typing={typing} onSave={saveWifi} />
      <Logs active={online && !!(features & FEATURE.LOGS)} buffer={logs} />
      <Help />
    </main>
  );
}
