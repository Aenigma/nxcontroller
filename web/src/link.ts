// The Web Bluetooth link to the board: framing, request/reply, typing flow control.
// Framework-free; the UI talks to it through the methods and callbacks below.
import {
  CAREFUL_HOLD_MS, CAREFUL_KEYS, FEATURE, FROM_DEVICE, FrameParser, KEYMAP, MAX_TYPE_KEYS, MSG,
  PROTOCOL_VERSION, SERVICE, STALL_TIMEOUT_MS, TO_DEVICE, TYPE_ROLLOVER, TYPE_WINDOW,
  errorText, frame, parseHelloAck,
  type DeviceInfo, type Frame,
} from "./protocol";

export interface LinkEvents {
  /** The connection dropped (not called while the board is restarting on purpose). */
  onDisconnect(): void;
  /** A LOG frame arrived (any time after LOGS was requested). */
  onLog?(payload: Uint8Array): void;
  /** The board is about to replay its log from the start (we asked to follow it): clear what's shown. */
  onLogReplay?(): void;
}

export interface TypeOptions {
  holdMs: number;
  rollover: boolean;
  onProgress(done: number, total: number): void;
  shouldStop(): boolean;
}

interface Pending {
  type: number;
  resolve(payload: Uint8Array): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export class Link {
  device: BluetoothDevice | undefined;
  info: DeviceInfo | undefined;
  /** Counts connections opened, including reconnects after a restart. */
  opened = 0;
  /** Whether the board should stream its log (see followLogs). */
  private logFollow = false;

  private to: BluetoothRemoteGATTCharacteristic | undefined;
  private from: BluetoothRemoteGATTCharacteristic | undefined;
  private readonly parser = new FrameParser();
  private pending: Pending | null = null;
  private asyncError: Error | null = null;
  private typed = 0;
  private progressWaiters: Array<() => void> = [];
  private chunk = 180; // ATT payload per write; drops to 20 if the link refuses
  private writing: Promise<void> = Promise.resolve();
  private restarting = false;
  private readonly onNotify = (e: Event) => {
    const v = (e.target as BluetoothRemoteGATTCharacteristic).value;
    if (v) this.onData(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
  };

  constructor(private readonly events: LinkEvents) {}

  async connect(): Promise<DeviceInfo> {
    this.device = await navigator.bluetooth.requestDevice({ filters: [{ services: [SERVICE] }] });
    this.device.addEventListener("gattserverdisconnected", () => {
      if (this.pending) { clearTimeout(this.pending.timer); this.pending.reject(new Error("disconnected")); this.pending = null; }
      this.wakeProgress();
      if (!this.restarting) this.events.onDisconnect();
    });
    return this.open();
  }

  async open(): Promise<DeviceInfo> {
    const device = this.device;
    if (!device?.gatt) throw new Error("no device");
    const server = await device.gatt.connect();
    const service = await server.getPrimaryService(SERVICE);
    this.to = await service.getCharacteristic(TO_DEVICE);
    this.from?.removeEventListener("characteristicvaluechanged", this.onNotify);
    this.from = await service.getCharacteristic(FROM_DEVICE);
    this.from.addEventListener("characteristicvaluechanged", this.onNotify);
    await this.from.startNotifications(); // encrypted characteristic: this triggers pairing
    this.parser.reset();
    const ack = await this.request(frame(MSG.HELLO, [PROTOCOL_VERSION]), MSG.HELLO_ACK);
    const info = parseHelloAck(ack);
    this.info = info;
    this.opened++;
    // A new connection (e.g. after a restart) while following: ask again.
    if (this.logFollow && info.features & FEATURE.LOGS) await this.sendLogs(true);
    return info;
  }

  get connected(): boolean { return !!this.device?.gatt?.connected; }
  get features(): number { return this.info?.features ?? 0; }
  disconnect(): void { this.device?.gatt?.disconnect(); }

  /** Feed bytes received from the board (exposed so tests can drive a simulated link). */
  onData(bytes: Uint8Array): void {
    for (const f of this.parser.push(bytes)) this.onFrame(f);
  }

  private onFrame({ type, payload }: Frame): void {
    if (type === MSG.LOG) {
      this.events.onLog?.(payload); // asynchronous: never part of a request/reply
    } else if (type === MSG.TYPE_PROGRESS) {
      this.typed += (payload[0] ?? 0) | ((payload[1] ?? 0) << 8);
      this.wakeProgress();
    } else if (this.pending && (type === this.pending.type || type === MSG.ERROR)) {
      const p = this.pending; this.pending = null; clearTimeout(p.timer);
      if (type === MSG.ERROR) p.reject(new Error(errorText(payload[0]))); else p.resolve(payload);
    } else if (type === MSG.ERROR) {
      this.asyncError = new Error(errorText(payload[0]));
      this.wakeProgress();
    }
  }

  private wakeProgress(): void { const w = this.progressWaiters; this.progressWaiters = []; w.forEach((f) => f()); }
  private waitProgress(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const t = setTimeout(resolve, ms);
      this.progressWaiters.push(() => { clearTimeout(t); resolve(); });
    });
  }

  /**
   * Send bytes (one or more frames) in order. Writes are acknowledged: the board can drop
   * unacknowledged ones when its buffers are full, and the browser never hears about it.
   */
  send(bytes: Uint8Array): Promise<void> {
    // Writes must stay in order; chain them.
    this.writing = this.writing.then(async () => {
      if (this.asyncError) { const e = this.asyncError; this.asyncError = null; throw e; }
      for (let i = 0; i < bytes.length;) {
        const part = bytes.slice(i, i + this.chunk);
        try {
          await this.to!.writeValueWithResponse(part);
          i += part.length;
        } catch (e) {
          if (this.chunk > 20 && this.connected) { this.chunk = 20; continue; } // small MTU: retry smaller
          throw e;
        }
      }
    });
    const done = this.writing;
    this.writing = this.writing.catch(() => {});
    return done;
  }

  request(bytes: Uint8Array, type: number, timeoutMs = 4000): Promise<Uint8Array> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending = null; reject(new Error("no reply from the board")); }, timeoutMs);
      this.pending = { type, resolve, reject, timer };
      this.send(bytes).catch((e) => { clearTimeout(timer); this.pending = null; reject(e); });
    });
  }

  async ping(): Promise<{ usbConfigured: boolean }> {
    const pong = await this.request(frame(MSG.PING, [1, 0, 0, 0]), MSG.PONG);
    return { usbConfigured: !!pong[4] };
  }

  async setMode(mode: number): Promise<void> {
    const ack = await this.request(frame(MSG.SET_MODE, [mode]), MSG.MODE_ACK);
    if (ack[1]) await this.reconnectAfterRestart();
  }

  async setWifi(ssid: string, password: string): Promise<void> {
    const enc = new TextEncoder(), s = enc.encode(ssid), p = enc.encode(password);
    await this.request(frame(MSG.SET_WIFI, [s.length, ...s, ...p]), MSG.WIFI_ACK);
    await this.reconnectAfterRestart();
  }

  /**
   * Whether to stream the board's log. Kept across reconnects; sent whenever the
   * board can act on it. Following replays everything the board has first. No ack.
   */
  async followLogs(on: boolean): Promise<void> {
    if (on === this.logFollow) return;
    this.logFollow = on;
    if (this.connected && this.features & FEATURE.LOGS) await this.sendLogs(on);
  }

  private async sendLogs(follow: boolean): Promise<void> {
    if (follow) this.events.onLogReplay?.();
    await this.send(frame(MSG.LOGS, [follow ? 1 : 0])).catch(() => {});
  }

  /** The board saved a setting and is restarting: wait for it and reconnect. */
  async reconnectAfterRestart(): Promise<void> {
    this.restarting = true;
    try {
      await sleep(2000);
      const deadline = Date.now() + 30000;
      for (;;) {
        try { await this.open(); return; }
        catch { if (Date.now() > deadline) throw new Error("the board didn't come back after restarting"); }
        await sleep(1000);
      }
    } finally {
      this.restarting = false;
    }
  }

  async releaseAll(): Promise<void> { await this.send(frame(MSG.RELEASE_ALL)); }

  /** Type `text`; the board paces every keystroke. Mirrors Controller.type_text. */
  async type(text: string, { holdMs, rollover, onProgress, shouldStop }: TypeOptions): Promise<boolean> {
    const keys = [...text].map((ch) => KEYMAP.get(ch)!);
    const total = keys.length;
    let sent = 0, lastDone = 0, lastChange = Date.now();
    this.typed = 0;
    for (;;) {
      if (shouldStop()) return false;
      if (this.asyncError) { const e = this.asyncError; this.asyncError = null; throw e; }
      if (!this.connected) throw new Error("disconnected");
      const done = this.typed, room = TYPE_WINDOW - (sent - done);
      if (done !== lastDone) { lastDone = done; lastChange = Date.now(); onProgress(Math.min(done, total), total); }
      if (done >= total) return true;
      if (sent < total && room > 0) {
        // Everything there's room for, as one stream of frames: fewer, fuller writes.
        const end = Math.min(sent + room, total), out: number[] = [];
        while (sent < end) {
          if (CAREFUL_KEYS.has(keys[sent]![1])) {
            // Release whatever rollover left down, then this key alone, held longer.
            const ms = Math.max(holdMs, CAREFUL_HOLD_MS);
            out.push(...frame(MSG.KEYBOARD, [0, 0, 0, 0, 0, 0, 0]), ...frame(MSG.TYPE, [0, ms, ms, ...keys[sent]!]));
            sent += 1;
            continue;
          }
          let n = Math.min(end - sent, MAX_TYPE_KEYS);
          for (let i = 0; i < n; i++) if (CAREFUL_KEYS.has(keys[sent + i]![1])) { n = i; break; }
          out.push(...frame(MSG.TYPE, [rollover ? TYPE_ROLLOVER : 0, holdMs, holdMs, ...keys.slice(sent, sent + n).flat()]));
          sent += n;
        }
        await this.send(Uint8Array.from(out));
        continue;
      }
      if (Date.now() - lastChange > STALL_TIMEOUT_MS) throw new Error(`typing stalled after ${done}/${total} keys`);
      await this.waitProgress(250);
    }
  }
}
