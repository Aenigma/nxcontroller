/** Text of the board's log, built from LOG frames (PROTOCOL.md, "Logs (LOGS)"). */

/** Longest text kept; older output is dropped past this. */
export const MAX_LOG_CHARS = 1_000_000;

export class LogBuffer {
  private decoder = new TextDecoder("utf-8");
  private chunks: string[] = [];
  private length = 0;
  private listeners = new Set<() => void>();
  /** Bumps on every change, so a view can tell whether it is current. */
  version = 0;

  /** Add a LOG payload. A multi-byte character may be split across frames. */
  push(payload: Uint8Array): void {
    const text = this.decoder.decode(payload, { stream: true });
    if (!text) return;
    this.chunks.push(text);
    this.length += text.length;
    if (this.length > MAX_LOG_CHARS) {
      const all = this.text.slice(this.length - MAX_LOG_CHARS);
      this.chunks = [all];
      this.length = all.length;
    }
    this.changed();
  }

  get text(): string {
    if (this.chunks.length > 1) this.chunks = [this.chunks.join("")];
    return this.chunks[0] ?? "";
  }

  /** Empty the log but keep decoding where it left off. */
  clear(): void {
    this.chunks = [];
    this.length = 0;
    this.changed();
  }

  /** A new connection: the board replays its log from the start, so begin afresh. */
  reset(): void {
    this.decoder = new TextDecoder("utf-8");
    this.clear();
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => { this.listeners.delete(fn); };
  }

  private changed(): void {
    this.version++;
    this.listeners.forEach((fn) => fn());
  }
}
