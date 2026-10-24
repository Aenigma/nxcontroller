export function Help() {
  return (
    <section>
      <h2>Help</h2>
      <details>
        <summary>Install says “Failed to initialize” or “Failed to open serial port”</summary>
        <ul>
          <li>You picked the <b>UART</b> chip's port (<i>USB Single Serial</i>, <i>CP210x</i>, <i>CH34x</i>). Pick <b>USB JTAG/serial debug unit</b>.</li>
          <li>The board left flashing mode. Reload the page, hold <kbd>BOOT</kbd>, tap <kbd>RST</kbd>, and keep holding <kbd>BOOT</kbd> until writing starts.</li>
        </ul>
      </details>
      <details>
        <summary>The board doesn't show up when connecting</summary>
        <ul>
          <li>Press <kbd>RST</kbd> after installing: until then it stays in flashing mode.</li>
          <li>On a Mac, allow Bluetooth for your browser in <b>System Settings → Privacy &amp; Security → Bluetooth</b>.</li>
          <li>Only one page or app can be connected at a time.</li>
        </ul>
      </details>
      <details>
        <summary>The Switch ignores the gamepad</summary>
        <p>Register it: <b>Controllers → Change Grip/Order</b>, then <b>Register</b>. Check the status line says USB is plugged in.</p>
      </details>
      <details>
        <summary>Typed text goes missing or lines get merged</summary>
        <p>Raise <b>Hold</b> (try 30&nbsp;ms) or turn off <b>Rollover</b>. Some apps only check the keyboard once per frame.
          Text only lands where typing is possible: on a Switch, a text field.</p>
      </details>
      <details>
        <summary>Seeing the board's log</summary>
        <p>While connected, the <b>Logs</b> section on this tab shows the board's recent output, live. Without a connection, plug
          the <b>UART</b> port into a computer and open it at 115200 baud (e.g. with <code>espflash monitor</code>),
          then press <kbd>RST</kbd>. The USB port never shows logs.</p>
      </details>
    </section>
  );
}
