// Settings → runtime state. Pure except for the palette module it configures.
import type { Settings } from "../state/backend";
import type { Metrics } from "../scene/metrics";
import { configureColors } from "../styles/palette";

const DAY_MS = 24 * 3600 * 1000;

export const AGE_PRESETS: [number, string][] = [
  [7, "1 week"],
  [30, "1 month"],
  [182, "6 months"],
  [365, "1 year"],
  [730, "2 years"],
  [1825, "5 years"],
];
export const ZOOM_PRESETS = [0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];
export const ANIMATION_PRESETS: [number, string][] = [
  [0, "off"],
  [120, "fast"],
  [240, "normal"],
  [400, "slow"],
];
export const TRUNCATE_PRESETS = [16, 25, 40, 60];
// Wheel delta per row: a smaller step scrolls faster.
export const WHEEL_PRESETS: [number, string][] = [
  [80, "slow"],
  [40, "normal"],
  [20, "fast"],
];

export const DEFAULTS: Settings = {
  color_mode: "age",
  age_max_days: 730,
  font: "iosevka",
  zoom: 1,
  animation_ms: 240,
  max_chars: 25,
  status_line: false,
  show_hidden: true,
  sort: "name",
  folders_first: false,
  wheel_step: 40,
  restore_last: true,
  last: null,
  recent_roots: [],
  respect_gitignore: true,
  excludes: [".git", "node_modules", "target", "AppData", "$Recycle.Bin", "System Volume Information"],
};

/** Metrics and palette values that follow from settings. */
export function applyToMetrics(s: Settings, m: Metrics, reducedMotion: boolean) {
  m.maxChars = s.max_chars;
  m.duration = reducedMotion ? 0 : s.animation_ms;
  configureColors({ mode: s.color_mode, ageMaxMs: s.age_max_days * DAY_MS });
}

export interface Changes {
  /** Listings must be fetched again (hidden files, sort order). */
  listing: boolean;
  /** The layout must be recomputed (truncation, colours). */
  layout: boolean;
  font: boolean;
  zoom: boolean;
  motion: boolean;
}

export function diff(prev: Settings, next: Settings): Changes {
  return {
    listing: prev.show_hidden !== next.show_hidden || prev.sort !== next.sort || prev.folders_first !== next.folders_first,
    layout: prev.max_chars !== next.max_chars || prev.color_mode !== next.color_mode || prev.age_max_days !== next.age_max_days,
    font: prev.font !== next.font,
    zoom: prev.zoom !== next.zoom,
    motion: prev.animation_ms !== next.animation_ms,
  };
}

/** Next/previous zoom step from the presets (Ctrl+= / Ctrl+-). */
export function stepZoom(zoom: number, dir: 1 | -1): number {
  const i = ZOOM_PRESETS.findIndex((z) => z >= zoom - 1e-6);
  const at = i < 0 ? ZOOM_PRESETS.length - 1 : i;
  const exact = Math.abs(ZOOM_PRESETS[at] - zoom) < 1e-6;
  const j = dir > 0 ? (exact ? at + 1 : at) : at - 1;
  return ZOOM_PRESETS[Math.max(0, Math.min(ZOOM_PRESETS.length - 1, j))];
}

/** "3 days ago", "just now", ... for the status line. */
export function relativeAge(mtime: number, now: number): string {
  const s = Math.max(0, (now - mtime) / 1000);
  const units: [number, string][] = [
    [365 * 86400, "year"],
    [30 * 86400, "month"],
    [7 * 86400, "week"],
    [86400, "day"],
    [3600, "hour"],
    [60, "minute"],
  ];
  for (const [secs, name] of units) {
    const n = Math.floor(s / secs);
    if (n >= 1) return `${n} ${name}${n > 1 ? "s" : ""} ago`;
  }
  return "just now";
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) (v /= 1024), i++;
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}
