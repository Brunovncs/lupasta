// Visual classification + colors. Everything color-related lives here.
//
// The reference does NOT color by extension (the same .swift folder shows four colors, and a
// folder flips from orange to blue depending on the column it is in). Colors are a recency
// ramp: white = just modified → gray → saturated = old. Columns on the selection path use the
// orange hue; preview columns use the blue hue. Kind-based colors are kept as an alternate mode.
import kindsConfig from "./file-kinds.json";

export type FileVisualKind = "directory" | "hidden" | "code" | "text" | "image" | "document" | "binary" | "special";
export type Hue = "path" | "preview";
export type ColorMode = "age" | "kind";

export const palette = {
  background: "#000000",
  selectedColor: "#e60000",
  connectorColor: "#8ab0ff",
  pathConnectorColor: "#f06c04",
  searchText: "#fcfcfc",
  searchDim: "#82808a",
  // kind mode
  directoryColor: "#fcfcfc",
  hiddenColor: "#82808a",
  codeColor: "#8ab0ff",
  textColor: "#b8b8b8",
  imageColor: "#6058a4",
  documentColor: "#f06c04",
  binaryColor: "#0000ec",
  specialColor: "#e07020",
};

// Age ramp stops [t, color], sampled from the reference frames (brightest glyph pixels).
// The blue ramp bends through violet: green fades faster than red on the way to #0000ec.
const RAMPS: Record<Hue, [number, string][]> = {
  path: [[0, "#fcfcfc"], [0.5, "#8a8886"], [1, "#f06c04"]],
  preview: [[0, "#fcfcfc"], [0.5, "#82808a"], [0.66, "#5c50a8"], [0.86, "#2410d4"], [1, "#0000ec"]],
};

export const colorMode: ColorMode = "age";

// Age curve: log-scale between AGE_MIN (t = 0) and AGE_MAX (t = 1).
export const AGE_MIN_MS = 10 * 60 * 1000;
export const AGE_MAX_MS = 2 * 365 * 24 * 3600 * 1000;
const LOG_SPAN = Math.log(AGE_MAX_MS / AGE_MIN_MS);

export function ageT(mtimeMs: number, now: number): number {
  const age = now - mtimeMs;
  if (!(age > AGE_MIN_MS)) return 0;
  return Math.min(1, Math.log(age / AGE_MIN_MS) / LOG_SPAN);
}

export function ageForT(t: number): number {
  return AGE_MIN_MS * Math.exp(t * LOG_SPAN);
}

const kindByExt = new Map<string, FileVisualKind>();
for (const [kind, exts] of Object.entries(kindsConfig.kinds)) for (const e of exts) kindByExt.set(e, kind as FileVisualKind);
const specialNames = new Set(kindsConfig.special);

export function extensionOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

export function classify(name: string, isDir: boolean, isHidden: boolean): FileVisualKind {
  if (specialNames.has(name)) return "special";
  if (isDir) return isHidden ? "hidden" : "directory";
  if (isHidden) return "hidden";
  return kindByExt.get(extensionOf(name)) ?? "binary";
}

const hex = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
const RAMP_RGB = Object.fromEntries(
  Object.entries(RAMPS).map(([k, stops]) => [k, stops.map(([t, c]) => [t, hex(c)] as const)]),
) as Record<Hue, (readonly [number, number[]])[]>;

export function rampColor(t: number, hue: Hue): string {
  const stops = RAMP_RGB[hue];
  const tt = Math.min(1, Math.max(0, t));
  let i = 1;
  while (i < stops.length - 1 && tt > stops[i][0]) i++;
  const [t0, from] = stops[i - 1];
  const [t1, to] = stops[i];
  const u = t1 > t0 ? (tt - t0) / (t1 - t0) : 0;
  const ch = (k: number) => Math.round(from[k] + (to[k] - from[k]) * u);
  return `rgb(${ch(0)},${ch(1)},${ch(2)})`;
}

export function colorFor(kind: FileVisualKind, t: number, hue: Hue, mode: ColorMode = colorMode): string {
  if (mode === "age") return rampColor(t, hue);
  return palette[`${kind}Color` as const];
}
