import { describe, expect, it, vi } from "vitest";
import { Link } from "./link";
import { LogBuffer } from "./logs";
import { MSG, frame } from "./protocol";

const utf8 = (s: string) => new TextEncoder().encode(s);

describe("LogBuffer", () => {
  it("concatenates chunks, even when a line is split across frames", () => {
    const b = new LogBuffer();
    b.push(utf8("boot"));
    b.push(utf8("ed\nready\n"));
    expect(b.text).toBe("booted\nready\n");
  });

  it("decodes a multi-byte character split across frames", () => {
    const bytes = utf8("…\nπ=3.14\n"); // starts like a truncated log
    const b = new LogBuffer();
    b.push(bytes.slice(0, 1));  // first byte of the ellipsis only
    expect(b.text).toBe("");
    b.push(bytes.slice(1, 4));  // the rest of it, plus "\n"
    b.push(bytes.slice(4, 5));  // first byte of π
    b.push(bytes.slice(5));
    expect(b.text).toBe("…\nπ=3.14\n");
  });

  it("clears, resets and notifies", () => {
    const b = new LogBuffer(), fn = vi.fn();
    b.subscribe(fn);
    b.push(utf8("a"));
    b.clear();
    expect(b.text).toBe("");
    b.push(utf8("b"));
    expect(b.text).toBe("b");
    b.reset();
    expect(b.text).toBe("");
    expect(fn).toHaveBeenCalledTimes(4);
  });
});

describe("Link on a simulated board", () => {
  function simulated() {
    const written: Uint8Array[] = [];
    const logs = new LogBuffer();
    const link = new Link({ onDisconnect() {}, onLog: (p) => logs.push(p) });
    (link as unknown as { to: unknown }).to = {
      writeValueWithoutResponse: async (v: Uint8Array) => { written.push(v); },
    };
    return { link, written, logs };
  }

  it("routes LOG frames to the listener, not to a pending request", async () => {
    const { link, logs } = simulated();
    const reply = link.request(frame(MSG.PING, [1, 0, 0, 0]), MSG.PONG);
    const log = frame(MSG.LOG, [...utf8("hello ")]);
    // A LOG frame in the middle of the exchange, split mid-frame, then the PONG.
    link.onData(log.slice(0, 4));
    link.onData(Uint8Array.from([...log.slice(4), ...frame(MSG.PONG, [1, 0, 0, 0, 1])]));
    link.onData(frame(MSG.LOG, [...utf8("world\n")]));
    expect([...(await reply)]).toEqual([1, 0, 0, 0, 1]);
    expect(logs.text).toBe("hello world\n");
  });

  it("asks for the log with follow = 1", async () => {
    const { link, written } = simulated();
    await link.requestLogs(true);
    await link.requestLogs(false);
    expect(written.map((w) => [...w])).toEqual([[MSG.LOGS, 1, 1], [MSG.LOGS, 1, 0]]);
  });
});
