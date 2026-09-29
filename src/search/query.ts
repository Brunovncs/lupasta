// Client-side query hygiene. Ranking itself happens in Rust (nucleo); this only decides what
// is worth sending and how a hit is split for display.
export function normalizeQuery(q: string): string {
  return q.split(/[\s/\\]+/).filter(Boolean).join(" ");
}

export function splitHit(path: string): { dir: string; name: string } {
  const i = path.lastIndexOf("/");
  return i < 0 ? { dir: "", name: path } : { dir: path.slice(0, i + 1), name: path.slice(i + 1) };
}

/** Keeps the tail of long parent paths, which is the informative part. */
export function clipDir(dir: string, max: number): string {
  const chars = Array.from(dir);
  return chars.length <= max ? dir : "…" + chars.slice(chars.length - max + 1).join("");
}
