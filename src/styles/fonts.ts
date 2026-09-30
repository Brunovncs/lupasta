// Font choice. The whole layout is measured in 10 px cells (metrics.charW), so a different
// monospace font is not allowed to change the cell: its size is set so that its advance is
// exactly one cell, its caps are centred where Iosevka's are, and "…" is squeezed into one cell.
import type { Metrics } from "../scene/metrics";

export interface FontChoice {
  id: string;
  label: string;
  /** CSS font-family list. */
  family: string;
}

export const FONTS: FontChoice[] = [
  { id: "iosevka", label: "Iosevka", family: '"Iosevka"' },
  { id: "jetbrains", label: "JetBrains Mono", family: '"JetBrains Mono"' },
  { id: "plex", label: "IBM Plex Mono", family: '"IBM Plex Mono"' },
  { id: "system", label: "system", family: '"Cascadia Mono", Consolas, "SF Mono", Menlo, "DejaVu Sans Mono", monospace' },
];

/** Iosevka at 20 px with textY -1.5 is the measured reference; everything else is fitted to it. */
const REFERENCE = { family: '"Iosevka"', fontSize: 20, textY: -1.5 };

export interface FontFit {
  fontSize: number;
  textY: number;
  ellScale: number;
}

export interface FontMetrics {
  /** Advance of one character, in em. */
  advance: number;
  /** Advance of "…", in em. */
  ellipsis: number;
  /** Font ascent/descent (what the line box uses) and cap height, in em. */
  ascent: number;
  descent: number;
  cap: number;
}

/** Pure: size and offset that put `f` in the reference cell. */
export function fitFont(f: FontMetrics, ref: FontMetrics, m: Pick<Metrics, "charW" | "rowH">): FontFit {
  const fontSize = m.charW / f.advance;
  const capCentre = (x: FontMetrics, size: number) => (m.rowH - (x.ascent + x.descent) * size) / 2 + x.ascent * size - (x.cap * size) / 2;
  const textY = REFERENCE.textY + capCentre(ref, REFERENCE.fontSize) - capCentre(f, fontSize);
  const ellScale = Math.min(1, m.charW / (f.ellipsis * fontSize));
  return { fontSize: round(fontSize), textY: round(textY), ellScale: round(ellScale) };
}

const round = (v: number) => Math.round(v * 1000) / 1000;

function measure(family: string): FontMetrics {
  const ctx = document.createElement("canvas").getContext("2d")!;
  ctx.font = `400 100px ${family}`;
  const m = ctx.measureText("M");
  const cap = ctx.measureText("H").actualBoundingBoxAscent;
  return {
    advance: m.width / 100,
    ellipsis: ctx.measureText("…").width / 100,
    ascent: m.fontBoundingBoxAscent / 100,
    descent: m.fontBoundingBoxDescent / 100,
    cap: cap / 100,
  };
}

async function loaded(family: string) {
  try {
    await document.fonts.load(`400 20px ${family}`, "M…H");
  } catch {
    // A system font that is not installed: measuring falls back to the next family in the list.
  }
}

/** Applies a font to the page and returns the metrics overrides for it. */
export async function applyFont(id: string, m: Metrics): Promise<FontFit> {
  const choice = FONTS.find((f) => f.id === id) ?? FONTS[0];
  const root = document.documentElement.style;
  let fit: FontFit;
  if (choice.id === "iosevka") {
    fit = { fontSize: REFERENCE.fontSize, textY: REFERENCE.textY, ellScale: 0.5 };
  } else {
    await Promise.all([loaded(REFERENCE.family), loaded(choice.family)]);
    fit = fitFont(measure(choice.family), measure(REFERENCE.family), m);
  }
  root.setProperty("--font", choice.family);
  root.setProperty("--font-size", `${fit.fontSize}px`);
  root.setProperty("--ell-scale", String(fit.ellScale));
  root.setProperty("--char-w", `${m.charW}px`);
  m.fontSize = fit.fontSize;
  m.textY = fit.textY;
  return fit;
}
