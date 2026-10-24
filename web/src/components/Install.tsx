import { useEffect, useState } from "preact/hooks";

export function Install() {
  // Firmware version on offer, from the installer manifest.
  const [version, setVersion] = useState("");
  useEffect(() => {
    fetch("manifest.json")
      .then((r) => r.json())
      .then((m: { version?: string }) => { if (m.version) setVersion(`Firmware ${m.version}`); })
      .catch(() => {});
  }, []);

  return (
    <section>
      <h2>1. Install the firmware</h2>
      <p class="muted intro">You need an ESP32-S3 board with a native USB port: most have two USB-C ports,
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
    </section>
  );
}
