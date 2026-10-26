// Per-browser conveniences (last tab, chosen board). Storage can be missing or
// blocked (private windows, previews): then nothing is remembered, that's all.

export function load(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

export function save(key: string, value: string): void {
  try { localStorage.setItem(key, value); } catch { /* not remembered */ }
}
