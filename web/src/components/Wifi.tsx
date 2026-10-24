import { useState } from "preact/hooks";

/** On GitHub Pages, link to the repository for the Python package. */
function pythonLink() {
  if (location.hostname.endsWith(".github.io")) {
    const user = location.hostname.split(".")[0], repo = location.pathname.split("/")[1];
    if (repo) return <a href={`https://github.com/${user}/${repo}#python-api`}>nxcontroller Python package</a>;
  }
  return <>nxcontroller Python package</>;
}

const byteLength = (v: string) => new TextEncoder().encode(v).length;

interface Props {
  /** The firmware has WiFi (HELLO feature bit): show the section. */
  active: boolean;
  online: boolean;
  typing: boolean;
  /** Save the network and wait for the board to come back. */
  onSave(ssid: string, password: string): Promise<void>;
}

export function Wifi({ active, online, typing, onSave }: Props) {
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState("");
  const [saving, setSaving] = useState(false);

  const save = async () => {
    if (!ssid || byteLength(ssid) > 32) { setStatus("The network name must be 1–32 bytes."); return; }
    if (password && (byteLength(password) < 8 || byteLength(password) > 64)) {
      setStatus("The password must be 8–64 characters, or empty for an open network.");
      return;
    }
    setStatus("Saving; the board restarts…");
    setSaving(true);
    try {
      await onSave(ssid, password);
      setStatus(`Saved. The board joins “${ssid}”.`);
      setPassword("");
    } catch (e) {
      setStatus("Failed: " + (e as Error).message);
    }
    setSaving(false);
  };

  return (
    <section hidden={!active}>
      <h2>WiFi</h2>
      <p class="muted intro">This board's firmware includes WiFi, for scripting it from the{" "}
        <span>{pythonLink()}</span> over your network. 2.4&nbsp;GHz networks only.
        The board saves the network and restarts.</p>
      <div class="row">
        <input type="text" placeholder="Network name" autocomplete="off" spellcheck={false}
          value={ssid} onInput={(e) => setSsid(e.currentTarget.value)} />
        <input type="password" placeholder="Password" autocomplete="off"
          value={password} onInput={(e) => setPassword(e.currentTarget.value)} />
        <button class="secondary" onClick={save} disabled={!online || typing || saving}>Save</button>
        <span class="muted">{status}</span>
      </div>
    </section>
  );
}
