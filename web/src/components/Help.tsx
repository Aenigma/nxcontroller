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
          In SmileBASIC, set <b>Editor</b> to <b>SmileBASIC</b>. Text only lands where typing is possible: on a Switch, a text field.</p>
      </details>
      <details>
        <summary>What the Editor setting does</summary>
        <p>Code editors add things as you type, so typing a file into one doesn't always give back the file.</p>
        <ul>
          <li><b>Normal</b> types the text as is. Use it for plain text fields and editors that don't indent.</li>
          <li><b>SmileBASIC</b> is for SmileBASIC 4, whose editor copies the previous line's indentation on Enter.
            Each line types only the change: extra spaces, or Backspaces to remove some. Tabs become spaces
            (<b>Tab</b> sets how many). Before an Enter that follows a word, it presses <kbd>End</kbd>: otherwise,
            when keyword suggestions are open (after <code>END</code>, say), Enter picks one and two lines merge.
            Start on an empty, unindented line.</li>
          <li><b>Strip indent</b> drops each line's leading spaces and tabs, for editors that indent for you, like VS Code.
            Editors that also close brackets and quotes can still leave extras: turn that off first
            (VS Code: <b>Auto Closing Brackets</b> and <b>Auto Closing Quotes</b> set to <i>never</i>).</li>
        </ul>
      </details>
      <details>
        <summary>Checking a program typed into SmileBASIC</summary>
        <p>The board can't see what SmileBASIC did with the keys, but SmileBASIC can check itself:</p>
        <ol>
          <li>Type the program into a slot with <b>Editor</b> set to <b>SmileBASIC</b>. Leave the text in the box.</li>
          <li>Pick that <b>Slot</b> and click <b>Verify slot</b>. The board presses <kbd>Ctrl</kbd>+<kbd>0</kbd> to switch to <b>DIRECT</b> mode, turns anything already
            on the input line into a comment, then types three short commands, a second apart, that clear the screen, read the slot and print
            two numbers: its length in characters, and a checksum.</li>
          <li>Compare them with the numbers next to the button. If both match, SmileBASIC holds exactly the text in the box.
            If not, something was lost or changed: type it again.</li>
        </ol>
        <p><b>Clear slot</b> erases the program in the slot the same way (<code>PRGEDIT 0:PRGDEL -1</code> for slot 0), so you can
          start typing into an empty program. It asks first, since it can't be undone.</p>
        <p>The commands leave the variables <code>S$</code>, <code>H</code> and <code>I</code> set. Empty lines at the end don't count.</p>
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
