// The settings panel as a flat list of rows, in the same one-line-per-entry language as the
// tree. Pure: the panel component renders these and routes keys/clicks back through them.
import type { AppInfo, IndexStatus, RootInfo, Settings, UpdateInfo } from "../state/backend";
import { FONTS } from "../styles/fonts";
import { AGE_PRESETS, ANIMATION_PRESETS, TRUNCATE_PRESETS, WHEEL_PRESETS, ZOOM_PRESETS } from "./prefs";

export type Action =
  | { type: "pick-root" }
  | { type: "up-root" }
  | { type: "open-root"; path: string }
  | { type: "reindex" }
  | { type: "clear-index" }
  | { type: "check-update" }
  | { type: "open-release"; url: string }
  | { type: "open-data" };

export interface Option {
  label: string;
  apply: (s: Settings) => Settings;
}

export type Row =
  | { kind: "head"; label: string }
  | { kind: "choice"; label: string; options: Option[]; index: number }
  | { kind: "action"; label: string; detail?: string; action: Action }
  | { kind: "edit"; label: string; value: string; commit: (s: Settings, text: string) => Settings }
  | { kind: "info"; label: string; detail: string };

export interface Context {
  settings: Settings;
  root: RootInfo | null;
  info: AppInfo | null;
  status: IndexStatus | null;
  update: UpdateInfo | { error: string } | "checking" | null;
}

function choice<T>(label: string, current: T, values: [T, string][], set: (s: Settings, v: T) => Settings): Row {
  const index = Math.max(0, values.findIndex(([v]) => v === current));
  return { kind: "choice", label, index, options: values.map(([v, l]) => ({ label: l, apply: (s) => set(s, v) })) };
}

const yesNo: [boolean, string][] = [
  [true, "on"],
  [false, "off"],
];

function nearest(values: number[], v: number) {
  return values.reduce((a, b) => (Math.abs(b - v) < Math.abs(a - v) ? b : a));
}

export function statusText(st: IndexStatus | null): string {
  if (!st) return "";
  if (st.enabled === false) return "off (--no-index)";
  const n = (st.indexed || st.corpus).toLocaleString("en-US");
  const t = st.state === "ready" && st.last_scan_ms ? `, last scan ${(st.last_scan_ms / 1000).toFixed(1)} s` : "";
  return `${st.state}, ${n} entries${t}`;
}

function updateText(u: Context["update"]): string {
  if (u === null) return "";
  if (u === "checking") return "checking…";
  if ("error" in u) return u.error;
  if (!u.latest) return "no releases published yet";
  return u.newer ? `${u.latest} available` : `up to date (${u.latest})`;
}

export const KEYS: [string, string][] = [
  ["↑ ↓  wheel", "move"],
  ["→  space", "enter folder"],
  ["←", "parent (at the top: go up a folder)"],
  ["enter  double-click", "open with the default app"],
  ["/  ctrl+k", "search"],
  ["ctrl+c", "copy the full path"],
  ["ctrl+e", "show in the file manager"],
  ["ctrl+h", "show / hide hidden files"],
  ["ctrl+o", "open another folder"],
  ["ctrl+=  ctrl+-  ctrl+0", "zoom"],
  ["ctrl+,", "settings"],
];

export function buildRows(c: Context): Row[] {
  const s = c.settings;
  const rows: Row[] = [];
  rows.push({ kind: "head", label: "appearance" });
  rows.push(choice("colour", s.color_mode, [["age", "by age"], ["kind", "by kind"]] as [Settings["color_mode"], string][], (x, v) => ({ ...x, color_mode: v })));
  rows.push(choice("oldest colour at", nearest(AGE_PRESETS.map(([d]) => d), s.age_max_days), AGE_PRESETS, (x, v) => ({ ...x, age_max_days: v })));
  rows.push(choice("font", s.font, FONTS.map((f) => [f.id, f.label] as [string, string]), (x, v) => ({ ...x, font: v })));
  rows.push(choice("zoom", nearest(ZOOM_PRESETS, s.zoom), ZOOM_PRESETS.map((z) => [z, `${Math.round(z * 100)}%`] as [number, string]), (x, v) => ({ ...x, zoom: v })));
  rows.push(choice("animation", s.animation_ms, ANIMATION_PRESETS, (x, v) => ({ ...x, animation_ms: v })));
  rows.push(choice("cut names at", s.max_chars, TRUNCATE_PRESETS.map((n) => [n, String(n)] as [number, string]), (x, v) => ({ ...x, max_chars: v })));
  rows.push(choice("status line", s.status_line, yesNo, (x, v) => ({ ...x, status_line: v })));

  rows.push({ kind: "head", label: "listing" });
  rows.push(choice("hidden files", s.show_hidden, [[true, "show"], [false, "hide"]], (x, v) => ({ ...x, show_hidden: v })));
  rows.push(choice("sort by", s.sort, [["name", "name"], ["modified", "newest"], ["size", "largest"]] as [Settings["sort"], string][], (x, v) => ({ ...x, sort: v })));
  rows.push(choice("folders first", s.folders_first, yesNo, (x, v) => ({ ...x, folders_first: v })));

  rows.push({ kind: "head", label: "navigation" });
  rows.push(choice("wheel speed", s.wheel_step, WHEEL_PRESETS, (x, v) => ({ ...x, wheel_step: v })));
  rows.push(choice("reopen last place", s.restore_last, yesNo, (x, v) => ({ ...x, restore_last: v })));

  rows.push({ kind: "head", label: "folder" });
  rows.push({ kind: "action", label: "open another…", detail: c.root?.abs ?? "", action: { type: "pick-root" } });
  if (c.root?.parent) rows.push({ kind: "action", label: "go up", detail: c.root.parent, action: { type: "up-root" } });
  for (const r of s.recent_roots.filter((r) => r !== c.root?.abs)) rows.push({ kind: "action", label: "recent", detail: r, action: { type: "open-root", path: r } });

  rows.push({ kind: "head", label: "search index" });
  rows.push({ kind: "info", label: "status", detail: statusText(c.status) });
  if (c.status?.watcher_error) rows.push({ kind: "info", label: "live updates", detail: `unavailable: ${c.status.watcher_error}` });
  rows.push(choice("respect .gitignore", s.respect_gitignore, yesNo, (x, v) => ({ ...x, respect_gitignore: v })));
  rows.push({
    kind: "edit",
    label: "skip folders",
    value: s.excludes.join(" "),
    commit: (x, text) => ({ ...x, excludes: text.split(/\s+/).filter(Boolean) }),
  });
  rows.push({ kind: "action", label: "rescan now", action: { type: "reindex" } });
  rows.push({ kind: "action", label: "rebuild from scratch", action: { type: "clear-index" } });

  rows.push({ kind: "head", label: "about" });
  rows.push({ kind: "info", label: "version", detail: c.info ? `${c.info.version} (${c.info.os})` : "" });
  const u = c.update;
  if (u && typeof u === "object" && "newer" in u && u.newer && u.url) {
    rows.push({ kind: "action", label: "download update", detail: updateText(u), action: { type: "open-release", url: u.url } });
  } else {
    rows.push({ kind: "action", label: "check for updates", detail: updateText(u), action: { type: "check-update" } });
  }
  rows.push({ kind: "action", label: "settings & index", detail: c.info?.data_dir ?? "", action: { type: "open-data" } });

  rows.push({ kind: "head", label: "keys" });
  for (const [k, what] of KEYS) rows.push({ kind: "info", label: k, detail: what });
  return rows;
}

export const focusable = (r: Row) => r.kind === "choice" || r.kind === "action" || r.kind === "edit";

/** Index of the next focusable row from `i` in direction `dir` (stays put at the ends). */
export function nextFocus(rows: Row[], i: number, dir: 1 | -1): number {
  for (let j = i + dir; j >= 0 && j < rows.length; j += dir) if (focusable(rows[j])) return j;
  return i;
}

/** The settings after moving a choice row's selection by `dir`, wrapping around. */
export function cycle(s: Settings, row: Row, dir: 1 | -1): Settings {
  if (row.kind !== "choice") return s;
  const n = row.options.length;
  return row.options[(row.index + dir + n) % n].apply(s);
}
