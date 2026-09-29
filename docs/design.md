# Design notes

The interface reproduces a 17-second screen recording (1812×1344, 120 fps) of a macOS file
browser prototype. The recording is not part of this repository. These are the rules that
were measured from its frames, and the places where lupasta departs from them.

## Layout

- The selection sits on a fixed horizontal line at the vertical centre of the window. Column
  *i* lists the siblings of the *i*-th folder on the path to the selection, shifted so that
  folder lands on that line.
- After the selection's column come previews: the contents of every folder among the
  selection's siblings, and, when the selection is a folder, the contents of its subfolders.
- Each preview group is centred on its parent's row. Groups are then stacked with one blank row
  between them, pushed outward from the group whose parent is closest to the focus line.
- Every name occupies its characters plus one trailing cell. Names are cut to 25 cells with an
  ellipsis, except the selected one. Long names in the selection's column do not widen the
  column; only the preview groups on their rows move right.
- Horizontally, the camera centres the bounding box of everything laid out, about 45 px right of
  the window centre.

## Connectors

- Always orthogonal, with small rounded elbows, 1.5 px wide, `#8ab0ff`. The path to the
  selection is orange.
- A connector meets its group at the row nearest the parent and runs straight when the group
  spans the parent's row. Elbowed connectors enter the group at its edge rather than at a row's
  centre.
- The vertical run goes next to the parent when nothing is in the way, otherwise next to the
  child column, in lanes ordered so that nested connectors do not cross.
- A connector whose target is off screen is drawn as a dot after the parent's name.

## Colour

Colour is a function of modification time, not file type: files of the same type appear in
four different colours in the recording, and a folder changes hue depending on which column
it is in. The ramp is continuous on a log scale from 10 minutes to two years: white, then grey,
then saturated orange (`#f06c04`) on the selection path or blue (`#0000ec`, through violet) in
previews. The stops were sampled from glyph pixels. A type-based palette is kept as an
alternative mode in `src/styles/palette.ts`, driven by the same `file-kinds.json` the Rust
index uses.

The selection is a red block (`#e60000`) behind the first character.

## Type

The glyph proportions (half-em advance, tall cap height) match Iosevka, which is bundled.
The Medium cut is used as the regular weight because Windows rasterises thinner than the
macOS text in the recording. Iosevka draws `…` two cells wide; the renderer squeezes it into
one, as in the original.

## Where lupasta differs

- **Animation.** The recording has none: every change happens within a single frame. lupasta
  interpolates positions and the camera over 240 ms. Set `duration: 0` in
  `src/scene/metrics.ts` to match the original.
- **Routing around long names.** The original threads connectors a few pixels apart around
  overhanging names and lands them on group edges. lupasta tries a detour and a landing route,
  and widens the column when neither fits. This is always crossing-free, but wider than the
  original in those states.
- **Second-level previews** use one uniform column; the original offsets some groups.
- **Search** has no counterpart in the recording and uses the same visual language.
