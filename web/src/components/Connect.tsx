export interface Status { msg: string; cls: string }

interface BarProps {
  online: boolean;
  unsupported: boolean;
  status: Status;
  onConnect(): void;
  onDisconnect(): void;
}

/** Connect / Disconnect and the status line, above the tabs. */
export function ConnectBar(p: BarProps) {
  return (
    <section class="connect-bar">
      <div class="row">
        <button onClick={p.onConnect} disabled={p.online || p.unsupported}>Connect</button>
        <button class="secondary" onClick={p.onDisconnect} disabled={!p.online}>Disconnect</button>
        <span class={p.status.cls}>{p.status.msg}</span>
      </div>
    </section>
  );
}

interface SettingsProps {
  online: boolean;
  /** Typing is under way: don't restart the board. */
  typing: boolean;
  applying: boolean;
  mode: string;
  onMode(mode: string): void;
  onApply(): void;
}

/** Step 2 of setup: connecting, and what the board acts as. */
export function BoardSettings(p: SettingsProps) {
  return (
    <section>
      <h2>2. Connect</h2>
      <ol>
        <li>Plug the board's <b>USB</b> port into what you want to control: the Switch dock or a computer.</li>
        <li>Click <b>Connect</b> at the top, choose <b>nxcontroller</b>, and accept the pairing request.</li>
      </ol>
      <div class="row mt14">
        <label>Acts as
          <select value={p.mode} onChange={(e) => p.onMode(e.currentTarget.value)} disabled={!p.online || p.typing}>
            <option value="3">gamepad + keyboard</option>
            <option value="1">gamepad</option>
            <option value="2">keyboard</option>
          </select>
        </label>
        <button class="secondary" onClick={p.onApply} disabled={!p.online || p.typing || p.applying}>Apply</button>
      </div>
      <p class="step-note">Changing this restarts the board, which reconnects by itself in a few seconds.</p>
    </section>
  );
}
