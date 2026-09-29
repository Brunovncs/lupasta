// Geometry tokens, in CSS px. The reference was captured at 2× (1812×1344 device px for a
// 906×672 window), so every value here is half of what is measured on the video frames.
export const metrics = {
  fontSize: 20, // Iosevka advance is 0.5em → 10 px per char
  charW: 10,
  rowH: 16,
  textY: -1.5, // text box offset inside its row (aligns baselines with the reference)
  maxChars: 25, // longer names are cut to 24 chars + "…" (except the selected one)
  cellPad: 1, // every name occupies one extra trailing cell (except a selected name past maxChars)
  pathGap: 0, // between columns, after the trailing cell
  previewGap: 9.5, // selection column → first preview column (room for connector lanes)
  groupGapRows: 1, // blank rows between stacked preview groups
  connectorY: 7.25, // where connectors meet a row, from the row top
  enterOffset: 5.25, // elbowed connectors enter a group at its edge, shifted toward the parent
  connGapL: 3, // gap between a name's last glyph and its outgoing connector
  connGapR: 2, // gap between an incoming connector and the child's first glyph
  nearParent: 5, // vertical run next to the parent (when nothing is in the way)
  nearChild: 8, // vertical run next to the child column
  laneGap: 3, // spacing between parallel vertical runs
  minRun: 3, // shortest horizontal stub before a vertical run
  landGap: 2, // a landing connector stops this far above/below the group's glyphs
  hook: 5, // length of the landing hook
  radius: 5, // elbow rounding
  strokeW: 1.5,
  pathStrokeW: 1.5,
  dotR: 1.5, // off-screen connector stub
  caretW: 10.5,
  caretX: -0.5,
  cameraBiasX: 45, // the reference centers the tree ~45 px right of the window center
  focusY: 0.5, // selection row top, as a fraction of the viewport height
  duration: 240, // ms; the reference snaps (0) — the spec asks for interpolation
  overscan: 64,
  windowRows: 120, // rows materialized above/below the focus row (virtualized layout)
};

export type Metrics = typeof metrics;
