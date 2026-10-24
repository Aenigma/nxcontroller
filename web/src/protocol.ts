// Mirrors protocol/PROTOCOL.md (and python/src/nxcontroller/protocol.py).

export const SERVICE = "a6240001-7a84-4692-ab2b-1fe245e78947";
export const TO_DEVICE = "a6240002-7a84-4692-ab2b-1fe245e78947";
export const FROM_DEVICE = "a6240003-7a84-4692-ab2b-1fe245e78947";
export const PROTOCOL_VERSION = 1;

export const MSG = {
  HELLO: 0x01, PING: 0x02, SET_MODE: 0x03, SET_WIFI: 0x04, LOGS: 0x06,
  GAMEPAD: 0x10, KEYBOARD: 0x11, TYPE: 0x12, RELEASE_ALL: 0x1f,
  HELLO_ACK: 0x81, PONG: 0x82, MODE_ACK: 0x83, WIFI_ACK: 0x84, TYPE_PROGRESS: 0x85, LOG: 0x87, ERROR: 0x8f,
} as const;

export const FEATURE = { GAMEPAD: 0x01, KEYBOARD: 0x02, LOGS: 0x20, WIFI: 0x40, PACED_TYPING: 0x80 } as const;

export const MODE_NAMES: Record<number, string> = { 1: "gamepad", 2: "keyboard", 3: "gamepad + keyboard" };

export const ERRORS: Record<number, string> = {
  1: "unknown message type", 2: "bad payload length", 3: "not available in the current USB mode",
  4: "protocol version mismatch", 5: "invalid value", 6: "failed to save setting", 7: "keystroke buffer full",
};

export const errorText = (code: number | undefined): string => "board: " + (ERRORS[code ?? 0] ?? "error " + code);

export const TYPE_ROLLOVER = 0x01;
export const MAX_TYPE_KEYS = 126;
export const TYPE_WINDOW = 256; // keystrokes allowed outstanding on the board
export const STALL_TIMEOUT_MS = 5000;
export const KEY_ENTER = 0x28;
export const LSHIFT = 0x02;
export const CAREFUL_KEYS: ReadonlySet<number> = new Set([KEY_ENTER]); // typed alone, after a release, held >= 20 ms
export const CAREFUL_HOLD_MS = 20;

export function frame(type: number, payload: ArrayLike<number> = []): Uint8Array {
  return Uint8Array.from([type, payload.length, ...Array.from(payload)]);
}

export interface Frame { type: number; payload: Uint8Array }

/** Reassembles frames from a byte stream (a frame may span several notifications). */
export class FrameParser {
  private buf: Uint8Array = new Uint8Array(0);

  push(bytes: Uint8Array): Frame[] {
    const joined = new Uint8Array(this.buf.length + bytes.length);
    joined.set(this.buf);
    joined.set(bytes, this.buf.length);
    const out: Frame[] = [];
    let at = 0;
    while (joined.length - at >= 2) {
      const len = joined[at + 1]!;
      if (joined.length - at < 2 + len) break;
      out.push({ type: joined[at]!, payload: joined.slice(at + 2, at + 2 + len) });
      at += 2 + len;
    }
    this.buf = joined.slice(at);
    return out;
  }

  reset(): void { this.buf = new Uint8Array(0); }
}

export interface DeviceInfo { version: number; features: number; name: string }

export function parseHelloAck(payload: Uint8Array): DeviceInfo {
  return {
    version: payload[0] ?? 0,
    features: payload[1] ?? 0,
    name: new TextDecoder().decode(payload.slice(2)),
  };
}

// US layout: character -> [modifiers, HID usage]
export type Key = readonly [modifiers: number, usage: number];

function buildKeymap(): Map<string, Key> {
  const map = new Map<string, Key>();
  for (let i = 0; i < 26; i++) {
    map.set(String.fromCharCode(97 + i), [0, 0x04 + i]);
    map.set(String.fromCharCode(65 + i), [LSHIFT, 0x04 + i]);
  }
  "1234567890".split("").forEach((d, i) => map.set(d, [0, 0x1e + i]));
  const plain: Record<string, number> = {
    "\n": 0x28, "\t": 0x2b, " ": 0x2c, "-": 0x2d, "=": 0x2e, "[": 0x2f, "]": 0x30,
    "\\": 0x31, ";": 0x33, "'": 0x34, "`": 0x35, ",": 0x36, ".": 0x37, "/": 0x38,
  };
  for (const [ch, key] of Object.entries(plain)) map.set(ch, [0, key]);
  const shifted = '!@#$%^&*()_+{}|:"~<>?', base = "1234567890-=[]\\;'`,./";
  shifted.split("").forEach((ch, i) => map.set(ch, [LSHIFT, map.get(base[i]!)![1]]));
  return map;
}
export const KEYMAP: ReadonlyMap<string, Key> = buildKeymap();

const REPLACEMENTS: Record<string, string> = {
  "‘": "'", "’": "'", "“": '"', "”": '"',
  "–": "-", "—": "--", "…": "...", " ": " ",
};

/** Normalise `text` to what a US keyboard can type; counts what had to be dropped. */
export function prepare(text: string): { text: string; skipped: number } {
  text = text.replace(/\r\n?/g, "\n");
  let out = "", skipped = 0;
  for (const ch of text) {
    const r = REPLACEMENTS[ch] ?? ch;
    for (const c of r) {
      if (KEYMAP.has(c)) out += c; else skipped++;
    }
  }
  return { text: out, skipped };
}
