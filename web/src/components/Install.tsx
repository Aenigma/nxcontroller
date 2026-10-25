import { useEffect, useState } from "preact/hooks";

/** Boards the page has firmware for. The remote works the same with each. */
export const BOARDS = [
  { id: "esp32s3", label: "ESP32-S3" },
  { id: "pico2w", label: "Raspberry Pi Pico 2 W", uf2: "firmware/nxcontroller-pico2-w.uf2", drive: "RP2350" },
  { id: "picow", label: "Raspberry Pi Pico W", uf2: "firmware/nxcontroller-pico-w.uf2", drive: "RPI-RP2" },
] as const;
type BoardId = (typeof BOARDS)[number]["id"];

const BOARD_KEY = "nxcontroller.board";

function savedBoard(): BoardId {
  try {
    const saved = localStorage.getItem(BOARD_KEY);
    if (BOARDS.some((b) => b.id === saved)) return saved as BoardId;
  } catch { /* storage unavailable: use the default */ }
  return "esp32s3";
}

export function Install() {
  const [board, setBoard] = useState<BoardId>(savedBoard);
  // Firmware version on offer, from the installer manifest (the same for every board).
  const [version, setVersion] = useState("");
  useEffect(() => {
    fetch("manifest.json")
      .then((r) => r.json())
      .then((m: { version?: string }) => { if (m.version) setVersion(`Firmware ${m.version}`); })
      .catch(() => {});
  }, []);

  const choose = (id: BoardId) => {
    setBoard(id);
    try { localStorage.setItem(BOARD_KEY, id); } catch { /* not remembered, that's all */ }
  };
  const chosen = BOARDS.find((b) => b.id === board)!;

  return (
    <section>
      <h2>1. Install the firmware</h2>
      <div class="row intro">
        <label>Board <select value={board} onChange={(e) => choose(e.currentTarget.value as BoardId)}>
          {BOARDS.map((b) => <option key={b.id} value={b.id}>{b.label}</option>)}
        </select></label>
      </div>
      {"uf2" in chosen ? <PicoSteps uf2={chosen.uf2} drive={chosen.drive} version={version} /> : <EspSteps version={version} />}
    </section>
  );
}

function EspSteps({ version }: { version: string }) {
  return (
    <>
      <p class="muted intro">The board needs a native USB port: most ESP32-S3 boards have two USB-C ports,
        labelled <b>USB</b> and <b>UART</b> (or COM).</p>
      <ol>
        <li>Plug the board's <b>USB</b> port into this computer.</li>
        <li>Hold <kbd>BOOT</kbd>, tap <kbd>RST</kbd>, release <kbd>BOOT</kbd>. This puts the board in flashing mode.
          Do it before every install: until then the port below doesn't appear.</li>
        <li>Click <b>Install</b> and pick <b>USB JTAG/serial debug unit</b>.
          <div class="row install-row">
            <esp-web-install-button manifest="manifest.json">
              <button slot="activate">Install</button>
              <span slot="unsupported" class="warn">Installing needs Chrome or Edge on a computer.</span>
              <span slot="not-allowed" class="warn">Installing needs this page over https.</span>
            </esp-web-install-button>
            <span class="muted">{version}</span>
          </div>
        </li>
        <li>When it finishes, press <kbd>RST</kbd> to start the firmware.</li>
      </ol>
      <p class="step-note">Tick <b>erase</b> only for a fresh start: it also clears the saved mode and WiFi network.</p>
    </>
  );
}

function PicoSteps({ uf2, drive, version }: { uf2: string; drive: string; version: string }) {
  const file = uf2.split("/").pop()!;
  return (
    <>
      <p class="muted intro">The Pico installs from a file: no special browser needed. Use a USB cable that carries data.</p>
      <ol>
        <li>Download the firmware.
          <div class="row install-row">
            <a class="button" href={uf2} download={file}>Download {file}</a>
            <span class="muted">{version}</span>
          </div>
        </li>
        <li>Hold the Pico's <kbd>BOOTSEL</kbd> button while you plug it into this computer, then let go.
          A drive called <b>{drive}</b> appears.</li>
        <li>Copy the file onto that drive. The Pico restarts into the firmware by itself and the drive goes away.</li>
      </ol>
      <p class="step-note">Updating the same way keeps the saved USB mode. The Pico has no WiFi in this firmware:
        it's controlled over Bluetooth, from this page.</p>
    </>
  );
}
